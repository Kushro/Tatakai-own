// SPDX-License-Identifier: AGPL-3.0-or-later
// Adapted from Enigmata (app.enigmata.temple.crypto.TempleCrypto / TempleCsr), same author, AGPL-3.0-or-later. See NOTICES-B4.md.
package app.tatakai.next.link

import android.annotation.SuppressLint
import okhttp3.OkHttpClient
import okhttp3.Protocol
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.math.BigInteger
import java.net.Socket
import java.security.GeneralSecurityException
import java.security.KeyFactory
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.MessageDigest
import java.security.Principal
import java.security.PrivateKey
import java.security.Signature
import java.security.cert.CertificateException
import java.security.cert.CertificateFactory
import java.security.cert.X509Certificate
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.PKCS8EncodedKeySpec
import java.util.Base64
import java.util.Date
import java.util.Locale
import java.util.concurrent.TimeUnit
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.KeyManager
import javax.net.ssl.SSLContext
import javax.net.ssl.SSLEngine
import javax.net.ssl.X509ExtendedKeyManager
import javax.net.ssl.X509TrustManager

/**
 * Key material and TLS for talking to a paired PC (review E4): a software P-256 key (sealed at rest by [LinkKeyVault], in memory only
 * behind [ClientKeyManager]), PEM/DER helpers, a trust manager pinned to the SHA-256 of the PC's CA, and the OkHttp clients for
 * `POST /v1/pairing` (no client certificate) and the mTLS routes. Exceptions carry fixed messages: never PEM, keys or certificates.
 */
object LinkCrypto {
    private const val SAN_IP_ADDRESS = 7
    private const val CLIENT_ALIAS = "tatakai-link-client"
    private const val EKU_SERVER_AUTH = "1.3.6.1.5.5.7.3.1"

    fun generateKeyPair(): KeyPair =
        KeyPairGenerator.getInstance("EC").run {
            initialize(ECGenParameterSpec("secp256r1"))
            generateKeyPair()
        }

    /** Unencrypted PKCS#8 `PRIVATE KEY` PEM (what the M1 mpv engine's `tls-key-file` will read). */
    fun privateKeyPem(key: PrivateKey): String {
        if (key.format != "PKCS#8") throw GeneralSecurityException("link: private key is not exportable as PKCS#8")
        return pem("PRIVATE KEY", key.encoded)
    }

    fun privateKeyFromPem(text: String): PrivateKey =
        KeyFactory.getInstance("EC").generatePrivate(PKCS8EncodedKeySpec(pemDer(text, "PRIVATE KEY")))

    fun certificateFromPem(text: String): X509Certificate = certificateFromDer(pemDer(text, "CERTIFICATE"))

    fun certificateFromDer(der: ByteArray): X509Certificate =
        CertificateFactory.getInstance("X.509").generateCertificate(ByteArrayInputStream(der)) as X509Certificate

    fun certificatePem(certificate: X509Certificate): String = pem("CERTIFICATE", certificate.encoded)

    /** The pin: lower-case hex SHA-256 of the certificate DER. */
    fun fingerprint(certificate: X509Certificate): String = sha256Hex(certificate.encoded)

    fun sha256Hex(bytes: ByteArray): String =
        MessageDigest.getInstance("SHA-256").digest(bytes).joinToString("") { "%02x".format(Locale.ROOT, it) }

    fun pem(label: String, der: ByteArray): String =
        buildString {
            append("-----BEGIN ").append(label).append("-----\n")
            Base64.getEncoder().encodeToString(der).chunked(64).forEach { append(it).append('\n') }
            append("-----END ").append(label).append("-----\n")
        }

    /** DER of the first [label] block in [text]; tolerates CRLF and surrounding text. */
    fun pemDer(text: String, label: String): ByteArray {
        val begin = "-----BEGIN $label-----"
        val end = "-----END $label-----"
        val start = text.indexOf(begin)
        val stop = if (start < 0) -1 else text.indexOf(end, start + begin.length)
        if (start < 0 || stop < 0) throw GeneralSecurityException("link: no $label PEM block")
        val body = text.substring(start + begin.length, stop).filterNot(Char::isWhitespace)
        return try {
            Base64.getDecoder().decode(body)
        } catch (_: IllegalArgumentException) {
            throw GeneralSecurityException("link: invalid $label PEM")
        }
    }

    /**
     * Accepts a server chain only when one of its certificates is the pinned CA (SHA-256 of the DER == [caSha256]) and the leaf
     * (`chain[0]`, a different certificate) was issued and signed by that CA, carries EKU serverAuth, is not a CA, and both are valid
     * at [now]. Nothing else is trusted, not even the system store. Never used for clients. Every failure path throws.
     */
    @SuppressLint("CustomX509TrustManager")
    class PinnedCaTrustManager(
        caSha256: String,
        private val now: () -> Date = { Date() },
    ) : X509TrustManager {
        private val pin = caSha256.lowercase(Locale.ROOT)

        override fun checkServerTrusted(chain: Array<out X509Certificate>?, authType: String?) {
            if (chain.isNullOrEmpty()) throw CertificateException("link: empty server chain")
            val ca = chain.firstOrNull { fingerprint(it) == pin }
                ?: throw CertificateException("link: server chain does not contain the pinned CA")
            val leaf = chain[0]
            if (leaf.encoded.contentEquals(ca.encoded)) throw CertificateException("link: server sent no leaf certificate")
            if (leaf.issuerX500Principal != ca.subjectX500Principal) throw CertificateException("link: leaf not issued by the pinned CA")
            try {
                leaf.verify(ca.publicKey)
            } catch (_: GeneralSecurityException) {
                throw CertificateException("link: leaf not signed by the pinned CA")
            }
            // The CA also issues client certificates (EKU clientAuth): only a serverAuth, non-CA leaf serves.
            if (leaf.extendedKeyUsage?.contains(EKU_SERVER_AUTH) != true) throw CertificateException("link: leaf is not a server certificate")
            if (leaf.basicConstraints != -1) throw CertificateException("link: leaf is a CA certificate")
            val at = now()
            ca.checkValidity(at)
            leaf.checkValidity(at)
        }

        override fun checkClientTrusted(chain: Array<out X509Certificate>?, authType: String?) {
            throw CertificateException("link: client certificates are not accepted here")
        }

        override fun getAcceptedIssuers(): Array<X509Certificate> = emptyArray()
    }

    /** Offers the paired client certificate whatever the server asks for; the PC decides. */
    class ClientKeyManager(
        private val key: PrivateKey,
        private val chain: Array<X509Certificate>,
    ) : X509ExtendedKeyManager() {
        override fun getClientAliases(keyType: String?, issuers: Array<out Principal>?) = arrayOf(CLIENT_ALIAS)
        override fun chooseClientAlias(keyType: Array<out String>?, issuers: Array<out Principal>?, socket: Socket?) = CLIENT_ALIAS
        override fun chooseEngineClientAlias(keyType: Array<out String>?, issuers: Array<out Principal>?, engine: SSLEngine?) = CLIENT_ALIAS
        override fun getServerAliases(keyType: String?, issuers: Array<out Principal>?): Array<String>? = null
        override fun chooseServerAlias(keyType: String?, issuers: Array<out Principal>?, socket: Socket?): String? = null
        override fun getCertificateChain(alias: String?): Array<X509Certificate>? = chain.takeIf { alias == CLIENT_ALIAS }
        override fun getPrivateKey(alias: String?): PrivateKey? = key.takeIf { alias == CLIENT_ALIAS }
    }

    /** The URL host must be exactly the paired IP literal, and the leaf must carry it as an IP SAN. */
    fun hostMatches(hostname: String, expectedHost: String, leaf: X509Certificate?): Boolean {
        if (hostname != expectedHost || leaf == null) return false
        val sans = runCatching { leaf.subjectAlternativeNames }.getOrNull() ?: return false
        return sans.any { it.size >= 2 && it[0] == SAN_IP_ADDRESS && it[1] == expectedHost }
    }

    fun hostnameVerifier(expectedHost: String): HostnameVerifier =
        HostnameVerifier { hostname, session ->
            val leaf = runCatching { session.peerCertificates.firstOrNull() as? X509Certificate }.getOrNull()
            hostMatches(hostname, expectedHost, leaf)
        }

    fun sslContext(trustManager: X509TrustManager, keyManager: KeyManager?): SSLContext =
        SSLContext.getInstance("TLS").apply {
            init(keyManager?.let { arrayOf(it) } ?: emptyArray(), arrayOf(trustManager), null)
        }

    /**
     * OkHttp client for the PC at [host]: HTTP/1.1 only (contract), pinned CA, exact host, no retries, no redirects. With [client] it
     * presents the client certificate (mTLS routes); without it, none (`POST /v1/pairing`).
     */
    fun httpClient(host: String, caSha256: String, client: ClientKeyManager? = null): OkHttpClient {
        val trustManager = PinnedCaTrustManager(caSha256)
        return OkHttpClient.Builder()
            .sslSocketFactory(sslContext(trustManager, client).socketFactory, trustManager)
            .hostnameVerifier(hostnameVerifier(host))
            .protocols(listOf(Protocol.HTTP_1_1))
            .retryOnConnectionFailure(false)
            .followRedirects(false)
            .followSslRedirects(false)
            .connectTimeout(10, TimeUnit.SECONDS)
            .readTimeout(20, TimeUnit.SECONDS)
            .callTimeout(30, TimeUnit.SECONDS)
            .build()
    }
}

/** Hand-written PKCS#10 for `POST /v1/pairing` (no ASN.1 library): subject `CN=<sanitized name>`, P-256 key, SHA256withECDSA. */
object LinkCsr {
    const val MAX_DEVICE_NAME_CHARS = 64
    const val FALLBACK_DEVICE_NAME = "unnamed device"

    private const val OID_EC_PUBLIC_KEY = "1.2.840.10045.2.1"
    private const val OID_P256 = "1.2.840.10045.3.1.7"
    private const val OID_ECDSA_SHA256 = "1.2.840.10045.4.3.2"
    private const val OID_COMMON_NAME = "2.5.4.3"
    private const val P256_COORDINATE_BYTES = 32

    fun createPem(keyPair: KeyPair, deviceName: String): String = LinkCrypto.pem("CERTIFICATE REQUEST", createDer(keyPair, deviceName))

    fun createDer(keyPair: KeyPair, deviceName: String): ByteArray {
        val publicKey = keyPair.public as? ECPublicKey ?: throw IllegalArgumentException("link csr: not an EC key")
        val info = Der.sequence(
            Der.integer(BigInteger.ZERO),
            Der.sequence(Der.set(Der.sequence(Der.oid(OID_COMMON_NAME), Der.utf8String(sanitizeDeviceName(deviceName))))),
            subjectPublicKeyInfo(publicKey),
            Der.tlv(0xa0, ByteArray(0)),
        )
        val signature = Signature.getInstance("SHA256withECDSA").run {
            initSign(keyPair.private)
            update(info)
            sign()
        }
        return Der.sequence(info, Der.sequence(Der.oid(OID_ECDSA_SHA256)), Der.bitString(signature))
    }

    fun subjectPublicKeyInfo(publicKey: ECPublicKey): ByteArray {
        require(publicKey.params.curve.field.fieldSize == 256) { "link csr: key is not P-256" }
        val point = byteArrayOf(0x04) + unsigned(publicKey.w.affineX, P256_COORDINATE_BYTES) + unsigned(publicKey.w.affineY, P256_COORDINATE_BYTES)
        return Der.sequence(Der.sequence(Der.oid(OID_EC_PUBLIC_KEY), Der.oid(OID_P256)), Der.bitString(point))
    }

    /** No control or format characters, whitespace collapsed, at most 64 characters, [FALLBACK_DEVICE_NAME] when nothing is left. */
    fun sanitizeDeviceName(name: String): String {
        val clean = StringBuilder()
        var pendingSpace = false
        var count = 0
        var i = 0
        while (i < name.length && count < MAX_DEVICE_NAME_CHARS) {
            val cp = name.codePointAt(i)
            i += Character.charCount(cp)
            if (Character.isWhitespace(cp) || Character.isSpaceChar(cp)) {
                pendingSpace = true
                continue
            }
            val type = Character.getType(cp)
            if (type == Character.CONTROL.toInt() || type == Character.FORMAT.toInt()) continue
            if (pendingSpace && clean.isNotEmpty()) {
                clean.append(' ')
                count++
                if (count == MAX_DEVICE_NAME_CHARS) break
            }
            pendingSpace = false
            clean.appendCodePoint(cp)
            count++
        }
        return clean.toString().trimEnd().ifEmpty { FALLBACK_DEVICE_NAME }
    }

    private fun unsigned(value: BigInteger, length: Int): ByteArray {
        val bytes = value.toByteArray()
        val trimmed = if (bytes.size > length && bytes[0] == 0.toByte()) bytes.copyOfRange(bytes.size - length, bytes.size) else bytes
        require(trimmed.size <= length) { "link csr: coordinate too large" }
        return ByteArray(length - trimmed.size) + trimmed
    }
}

/** Minimal DER encoder: only what the CSR needs. */
internal object Der {
    fun tlv(tag: Int, content: ByteArray): ByteArray {
        val out = ByteArrayOutputStream(content.size + 6)
        out.write(tag)
        when {
            content.size < 0x80 -> out.write(content.size)
            content.size <= 0xff -> { out.write(0x81); out.write(content.size) }
            content.size <= 0xffff -> { out.write(0x82); out.write(content.size shr 8); out.write(content.size and 0xff) }
            else -> { out.write(0x83); out.write(content.size shr 16); out.write((content.size shr 8) and 0xff); out.write(content.size and 0xff) }
        }
        out.write(content)
        return out.toByteArray()
    }

    fun sequence(vararg parts: ByteArray): ByteArray = tlv(0x30, concat(parts))
    fun set(vararg parts: ByteArray): ByteArray = tlv(0x31, concat(parts))
    fun integer(value: BigInteger): ByteArray = tlv(0x02, value.toByteArray())
    fun bitString(bytes: ByteArray): ByteArray = tlv(0x03, byteArrayOf(0) + bytes)
    fun utf8String(text: String): ByteArray = tlv(0x0c, text.toByteArray(Charsets.UTF_8))

    fun oid(dotted: String): ByteArray {
        val arcs = dotted.split('.').map(String::toLong)
        require(arcs.size >= 2 && arcs[0] in 0..2) { "bad oid" }
        val out = ByteArrayOutputStream()
        writeBase128(out, arcs[0] * 40 + arcs[1])
        arcs.drop(2).forEach { writeBase128(out, it) }
        return tlv(0x06, out.toByteArray())
    }

    private fun writeBase128(out: ByteArrayOutputStream, value: Long) {
        require(value >= 0) { "bad oid arc" }
        val groups = ArrayDeque<Int>()
        var v = value
        do {
            groups.addFirst((v and 0x7f).toInt())
            v = v shr 7
        } while (v != 0L)
        groups.forEachIndexed { index, group -> out.write(if (index < groups.size - 1) group or 0x80 else group) }
    }

    private fun concat(parts: Array<out ByteArray>): ByteArray {
        val out = ByteArrayOutputStream(parts.sumOf { it.size })
        parts.forEach { out.write(it) }
        return out.toByteArray()
    }
}
