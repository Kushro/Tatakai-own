// SPDX-License-Identifier: AGPL-3.0-or-later
// Adapted from Enigmata (app.enigmata.temple.client.TemplePairingClient), same author, AGPL-3.0-or-later. See NOTICES-B4.md.
package app.tatakai.next.link

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.withContext
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody
import java.security.GeneralSecurityException
import java.security.KeyPair
import java.security.cert.X509Certificate
import java.security.interfaces.ECPublicKey

/** What `POST /v1/pairing` grants, already checked against the payload's pin and the phone's key. */
class PairingGrant(val deviceId: String, val certificate: X509Certificate, val ca: X509Certificate) {
    override fun toString(): String = "PairingGrant(device=${deviceId.take(8)}...)"
}

/**
 * `POST /v1/pairing`: TLS pinned to the payload's CA, no client certificate; body `{code, device_name, csr_pem, platform:"android"}`.
 * The `201` is accepted only if the returned CA is the pinned one, the certificate carries the phone's public key and is signed by that
 * CA, and `device_id` is a UUID; anything else is `Unexpected(BODY)`. Nothing is logged: code, CSR and PEM are secrets.
 *
 * [clientFor] builds the HTTP client for the payload (default: pinned-CA TLS); tests inject plain HTTP.
 */
class PairingClient(
    private val clientFor: (PairingPayload) -> OkHttpClient = { LinkCrypto.httpClient(it.host, it.caSha256) },
    private val baseUrlFor: (PairingPayload) -> HttpUrl = { it.baseUrl.toHttpUrl() },
    private val io: CoroutineDispatcher = Dispatchers.IO,
) {
    suspend fun pair(payload: PairingPayload, deviceName: String, keyPair: KeyPair): LinkResult<PairingGrant> {
        val name = LinkCsr.sanitizeDeviceName(deviceName)
        val body = buildJsonObject {
            put("code", payload.code)
            put("device_name", name)
            put("csr_pem", LinkCsr.createPem(keyPair, name))
            put("platform", "android")
        }.toString().toByteArray(Charsets.UTF_8)
        if (body.size > MAX_PAIRING_BODY) return LinkResult.Failure(LinkError.Api(LinkApiError.BodyTooLarge))

        val client = clientFor(payload)
        val url = baseUrlFor(payload).newBuilder().addPathSegments("v1/pairing").build()
        val request = LinkHttp.request(url).post(body.toRequestBody(LinkHttp.JSON_TYPE)).build()
        return try {
            LinkHttp.exchange(client, request, MAX_RESPONSE_BYTES) { _, response ->
                grant(LinkHttp.json.decodeFromString(Issued.serializer(), response.decodeToString()), payload, keyPair)
            }
        } finally {
            // Closing the pooled TLS socket writes close_notify, which must not happen on the main thread.
            withContext(NonCancellable + io) { client.connectionPool.evictAll() }
        }
    }

    private fun grant(issued: Issued, payload: PairingPayload, keyPair: KeyPair): PairingGrant {
        require(DEVICE_ID.matches(issued.deviceId))
        try {
            val ca = LinkCrypto.certificateFromPem(issued.caCertPem)
            require(LinkCrypto.fingerprint(ca) == payload.caSha256)
            val certificate = LinkCrypto.certificateFromPem(issued.clientCertPem)
            val certificateKey = certificate.publicKey as? ECPublicKey
            require(certificateKey != null && certificateKey.w == (keyPair.public as ECPublicKey).w)
            certificate.verify(ca.publicKey)
            return PairingGrant(issued.deviceId, certificate, ca)
        } catch (_: GeneralSecurityException) {
            throw IllegalArgumentException("link: pairing answer does not verify")
        }
    }

    @Serializable
    private class Issued(
        @SerialName("device_id") val deviceId: String,
        @SerialName("client_cert_pem") val clientCertPem: String,
        @SerialName("ca_cert_pem") val caCertPem: String,
    )

    private companion object {
        const val MAX_PAIRING_BODY = 16 * 1024
        const val MAX_RESPONSE_BYTES = 64L * 1024
        val DEVICE_ID = Regex("""[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}""")
    }
}
