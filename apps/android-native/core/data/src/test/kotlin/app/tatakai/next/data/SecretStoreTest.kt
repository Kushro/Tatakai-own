// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.link.AesGcmKeyCipher
import app.tatakai.next.model.SecretKeys
import kotlinx.coroutines.runBlocking
import org.junit.Rule
import org.junit.rules.TemporaryFolder
import java.io.File
import javax.crypto.KeyGenerator
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull

/** parity wave A (worker q, system.secrets-store): sealed file, survives a new instance, never stores plain text. */
class SecretStoreTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private val key = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()

    @Test
    fun putGetClearAndPersistence() = runBlocking {
        val store = KeystoreSecretStore(tmp.root, AesGcmKeyCipher { key })
        val k = SecretKeys.debridApiKey("real_debrid")
        store.put(k, "SECRET-123")
        assertEquals("SECRET-123", store.get(k))
        val bytes = tmp.root.listFiles().orEmpty().filter(File::isFile).flatMap { it.readBytes().toList() }.toByteArray()
        assertFalse(String(bytes, Charsets.ISO_8859_1).contains("SECRET-123"), "the key must be sealed on disk")
        assertEquals("SECRET-123", KeystoreSecretStore(tmp.root, AesGcmKeyCipher { key }).get(k))
        store.put(k, null)
        assertNull(store.get(k))
        store.put("a", "1")
        store.clear()
        assertNull(KeystoreSecretStore(tmp.root, AesGcmKeyCipher { key }).get("a"))
    }

    @Test
    fun aFileSealedWithAnotherKeyReadsAsEmpty() = runBlocking {
        KeystoreSecretStore(tmp.root, AesGcmKeyCipher { key }).put("x", "1")
        val other = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
        assertNull(KeystoreSecretStore(tmp.root, AesGcmKeyCipher { other }).get("x"))
    }

    @Test
    fun secretKeyNames() {
        assertEquals("list.anilist.token", SecretKeys.listToken("anilist"))
        assertEquals("debrid.torbox.api_key", SecretKeys.debridApiKey("torbox"))
    }
}
