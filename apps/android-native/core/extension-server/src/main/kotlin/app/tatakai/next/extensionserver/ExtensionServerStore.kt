// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import app.tatakai.next.link.AesGcmKeyCipher
import app.tatakai.next.link.KeyCipher
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.io.File
import java.io.IOException
import java.security.KeyStore
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey

/**
 * The linked servers of this phone, tokens included, sealed at rest with [cipher] (AES-256-GCM under an AndroidKeyStore key in
 * production, see [ExtensionServerKey]) in one file under `noBackupFilesDir`. A missing, damaged or foreign file reads as "no servers".
 * Writes go through a temporary file + rename so a crash never leaves half a file.
 */
class ExtensionServerStore(
    private val file: File,
    private val cipher: KeyCipher,
    private val json: Json = ExtensionServerClient.DefaultJson,
) {
    @Serializable
    private data class Stored(val version: Int = VERSION, val servers: List<ExtensionServerConnection> = emptyList())

    @Synchronized
    fun read(): List<ExtensionServerConnection> {
        if (!file.isFile) return emptyList()
        return try {
            val plain = cipher.open(file.readBytes()) ?: return emptyList()
            json.decodeFromString(Stored.serializer(), plain.decodeToString()).servers
                .filter { HostGuard.isAllowed(it.url) && it.token.isNotEmpty() && it.id.isNotEmpty() }
        } catch (_: Exception) {
            emptyList()
        }
    }

    @Synchronized
    fun write(servers: List<ExtensionServerConnection>) {
        file.parentFile?.mkdirs()
        val sealed = cipher.seal(json.encodeToString(Stored.serializer(), Stored(servers = servers)).toByteArray())
        val tmp = File(file.parentFile, file.name + ".tmp")
        tmp.writeBytes(sealed)
        if (!tmp.renameTo(file)) {
            file.delete()
            if (!tmp.renameTo(file)) throw IOException("cannot store extension servers")
        }
    }

    private companion object {
        const val VERSION = 1
    }
}

/** The non-exportable AES key (alias [ALIAS]) of [ExtensionServerStore]; created on first use. Separate from the Link client's key. */
object ExtensionServerKey {
    private const val ANDROID_KEYSTORE = "AndroidKeyStore"
    const val ALIAS = "tatakai_extension_server_token_seal"

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
