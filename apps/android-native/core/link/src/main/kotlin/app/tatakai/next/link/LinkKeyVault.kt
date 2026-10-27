// SPDX-License-Identifier: AGPL-3.0-or-later
// Model adapted from Enigmata (app.enigmata.temple.vault.TempleKeyVault / KeystoreTempleKeyCipher), same author, AGPL-3.0-or-later.
package app.tatakai.next.link

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import java.security.PrivateKey
import java.security.SecureRandom
import java.security.cert.X509Certificate
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/** Seals/opens small secrets at rest. [open] returns null for anything that does not authenticate (never plaintext). */
interface KeyCipher {
    fun seal(plain: ByteArray): ByteArray
    fun open(sealed: ByteArray): ByteArray?
}

/** AES-256-GCM, output = `MAGIC || iv(12) || ciphertext+tag`. The key comes from [keyProvider] (AndroidKeyStore in production). */
class AesGcmKeyCipher(private val keyProvider: () -> SecretKey) : KeyCipher {
    override fun seal(plain: ByteArray): ByteArray {
        val iv = ByteArray(IV_BYTES).also { SecureRandom().nextBytes(it) }
        val cipher = Cipher.getInstance(TRANSFORMATION)
        // AndroidKeyStore keys refuse caller-supplied IVs when randomized encryption is required: let the provider generate it.
        cipher.init(Cipher.ENCRYPT_MODE, keyProvider())
        val generated = cipher.iv ?: iv
        val ct = cipher.doFinal(plain)
        return MAGIC + generated + ct
    }

    override fun open(sealed: ByteArray): ByteArray? {
        if (sealed.size < MAGIC.size + IV_BYTES + TAG_BYTES || !sealed.copyOfRange(0, MAGIC.size).contentEquals(MAGIC)) return null
        return try {
            val iv = sealed.copyOfRange(MAGIC.size, MAGIC.size + IV_BYTES)
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(Cipher.DECRYPT_MODE, keyProvider(), GCMParameterSpec(TAG_BYTES * 8, iv))
            cipher.doFinal(sealed, MAGIC.size + IV_BYTES, sealed.size - MAGIC.size - IV_BYTES)
        } catch (_: GeneralSecurityException) {
            null
        }
    }

    companion object {
        private const val TRANSFORMATION = "AES/GCM/NoPadding"
        private const val IV_BYTES = 12
        private const val TAG_BYTES = 16
        private val MAGIC = "TKL1".toByteArray(Charsets.US_ASCII)
    }
}

/** The non-exportable AES key (alias [ALIAS]) living in AndroidKeyStore; created on first use. */
object AndroidKeystoreKey {
    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    const val ALIAS = "tatakai_link_client_key_seal"

    fun cipher(): KeyCipher = AesGcmKeyCipher(::secretKey)

    private fun secretKey(): SecretKey {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
        (keyStore.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .setKeySize(256)
                .build(),
        )
        return generator.generateKey()
    }
}

@Serializable
data class StoredPairing(val deviceId: String, val serverName: String, val host: String, val port: Int, val caSha256: String)

/** Everything needed to talk to the paired PC, loaded into memory. [toString] reveals nothing secret. */
class LinkIdentity(
    val pairing: StoredPairing,
    val privateKey: PrivateKey,
    val certificate: X509Certificate,
    val ca: X509Certificate,
) {
    override fun toString(): String = "LinkIdentity(device=${pairing.deviceId.take(8)}...)"
}

/**
 * The paired identity at rest (review E4): the software P-256 key is sealed with [cipher] and stored under `<baseDir>/link/`
 * (production: `noBackupFilesDir`, outside backup/export). Non-secret material (pairing metadata, certificates) is stored plain.
 * A stored key that does not authenticate or parse reads as "unpaired". Only the five fixed files of this directory are ever deleted.
 * Nothing is logged; no message contains key, PEM or certificate content.
 */
class LinkKeyVault(baseDir: File, private val cipher: KeyCipher) {
    private val dir = File(baseDir, DIR)

    fun save(pairing: StoredPairing, clientKeyPem: String, clientCertPem: String, caPem: String) {
        val sealed = cipher.seal(clientKeyPem.toByteArray(Charsets.UTF_8))
        if (sealed.isEmpty()) throw GeneralSecurityException("link: client key could not be sealed")
        dir.mkdirs()
        restrict(dir, executable = true)
        write(KEY_FILE, sealed)
        write(CERT_FILE, clientCertPem.toByteArray(Charsets.UTF_8))
        write(CA_FILE, caPem.toByteArray(Charsets.UTF_8))
        write(PAIRING_FILE, Json.encodeToString(StoredPairing.serializer(), pairing).toByteArray(Charsets.UTF_8))
    }

    fun load(): LinkIdentity? {
        return try {
            val pairing = Json.decodeFromString(StoredPairing.serializer(), File(dir, PAIRING_FILE).readText())
            val keyPem = cipher.open(File(dir, KEY_FILE).readBytes())?.toString(Charsets.UTF_8) ?: return null
            LinkIdentity(
                pairing,
                LinkCrypto.privateKeyFromPem(keyPem),
                LinkCrypto.certificateFromPem(File(dir, CERT_FILE).readText()),
                LinkCrypto.certificateFromPem(File(dir, CA_FILE).readText()),
            )
        } catch (_: Exception) {
            null
        }
    }

    /** Non-secret pairing metadata only (no Keystore access); null when unpaired or unreadable. */
    fun pairing(): StoredPairing? =
        runCatching { Json.decodeFromString(StoredPairing.serializer(), File(dir, PAIRING_FILE).readText()) }.getOrNull()

    fun hasPairing(): Boolean = File(dir, PAIRING_FILE).isFile

    /** Forgets the pairing: deletes the fixed set of files, nothing else. */
    fun clear() {
        listOf(KEY_FILE, CERT_FILE, CA_FILE, PAIRING_FILE).forEach { File(dir, it).delete() }
    }

    /** Raw bytes of the sealed key file (tests assert it is not plaintext). */
    internal fun sealedKeyBytes(): ByteArray? = File(dir, KEY_FILE).takeIf { it.isFile }?.readBytes()

    private fun write(name: String, bytes: ByteArray) {
        val target = File(dir, name)
        val tmp = File(dir, "$name.tmp")
        tmp.writeBytes(bytes)
        restrict(tmp, executable = false)
        if (!tmp.renameTo(target)) {
            target.delete()
            if (!tmp.renameTo(target)) throw IOException("link: could not store $name")
        }
    }

    private fun restrict(file: File, executable: Boolean) {
        file.setReadable(false, false)
        file.setWritable(false, false)
        file.setExecutable(false, false)
        file.setReadable(true, true)
        file.setWritable(true, true)
        if (executable) file.setExecutable(true, true)
    }

    companion object {
        const val DIR = "link"
        const val KEY_FILE = "client.key.sealed"
        const val CERT_FILE = "client.cert.pem"
        const val CA_FILE = "ca.pem"
        const val PAIRING_FILE = "pairing.json"
    }
}
