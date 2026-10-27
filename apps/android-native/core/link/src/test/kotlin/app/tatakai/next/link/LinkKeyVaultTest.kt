// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import java.nio.file.Files
import javax.crypto.KeyGenerator
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class LinkKeyVaultTest {
    private val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    private val cipher = AesGcmKeyCipher { secret }

    @Test
    fun cipherRoundTripsAndRejectsTampering() {
        val sealed = cipher.seal("hello".toByteArray())
        assertEquals("hello", cipher.open(sealed)?.decodeToString())
        sealed[sealed.size - 1] = (sealed[sealed.size - 1].toInt() xor 1).toByte()
        assertNull(cipher.open(sealed))
        assertNull(cipher.open("plain text, not sealed at all.....".toByteArray()))
        val other = AesGcmKeyCipher { KeyGenerator.getInstance("AES").apply { init(256) }.generateKey() }
        assertNull(other.open(cipher.seal("x".toByteArray())))
    }

    @Test
    fun keyIsSealedAtRestAndGarbageReadsAsUnpaired() {
        val base = Files.createTempDirectory("tatakai-vault-test").toFile()
        try {
            val vault = LinkKeyVault(base, cipher)
            val pair = LinkCrypto.generateKeyPair()
            val keyPem = LinkCrypto.privateKeyPem(pair.private)
            assertTrue(LinkCsr.createPem(pair, "My Phone").startsWith("-----BEGIN CERTIFICATE REQUEST-----"))
            assertEquals("My Phone", LinkCsr.sanitizeDeviceName("  My\u0000   Phone "))
            assertEquals(LinkCsr.FALLBACK_DEVICE_NAME, LinkCsr.sanitizeDeviceName(" \u0000 "))

            assertNull(vault.load())
            assertFalse(vault.hasPairing())
            vault.save(StoredPairing("d", "PC", "10.0.0.2", 47321, "ab".repeat(32)), keyPem, "not a cert", "not a cert")
            val sealed = assertNotNull(vault.sealedKeyBytes())
            assertFalse(sealed.decodeToString().contains("PRIVATE KEY"), "the key must not be stored as PEM")
            assertEquals("10.0.0.2", vault.pairing()?.host)
            assertNull(vault.load(), "garbage certificates must read as unpaired, not crash")
            vault.clear()
            assertFalse(vault.hasPairing())
        } finally {
            base.deleteRecursively() // temp dir created above by createTempDirectory
        }
    }
}
