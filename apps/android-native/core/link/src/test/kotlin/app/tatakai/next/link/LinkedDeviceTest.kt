// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.test.runTest
import java.nio.file.Files
import javax.crypto.KeyGenerator
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull

/** android wave (12 §3.5 / §3.6): LinkRepository.linkedDevice and setPreferredProfile over the vault's device.json. */
class LinkedDeviceTest {
    private val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    private val cipher = AesGcmKeyCipher { secret }

    @Test
    fun fingerprintIsGroupedForDisplay() {
        assertEquals("7A3F 91C2 AB", LinkRepositoryImpl.displayFingerprint("7a3f91c2ab"))
        assertNull(LinkRepositoryImpl.displayFingerprint(""))
    }

    @Test
    fun linkedDeviceComesFromPairingAndMeta() = runTest {
        val base = Files.createTempDirectory("tatakai-linked-test").toFile()
        try {
            val vault = LinkKeyVault(base, cipher)
            assertNull(LinkRepositoryImpl(vault = vault, io = Dispatchers.Unconfined).linkedDevice.value)

            val pair = LinkCrypto.generateKeyPair()
            vault.save(StoredPairing("dev-1", "Octavio-PC", "192.168.1.20", 47321, "ab".repeat(32)), LinkCrypto.privateKeyPem(pair.private), "x", "x")
            vault.saveMeta(StoredDeviceMeta(pairedAtMs = 1000, lastSeenMs = 2000, localDeviceName = "Pixel"))
            val repo = LinkRepositoryImpl(vault = vault, io = Dispatchers.Unconfined)
            val d = repo.linkedDevice.value!!
            assertEquals("Octavio-PC", d.serverName)
            assertEquals("192.168.1.20", d.host)
            assertEquals(47321, d.port)
            assertEquals(1000L, d.pairedAtMs)
            assertEquals(2000L, d.lastSeenMs)
            assertEquals("Pixel", d.localDeviceName)
            assertEquals(List(16) { "ABAB" }.joinToString(" "), d.fingerprint)
            assertNull(d.preferredProfile)

            repo.setPreferredProfile("quality")
            assertEquals("quality", repo.linkedDevice.value?.preferredProfile)
            assertEquals("quality", vault.meta()?.preferredProfile)
            assertEquals(1000L, vault.meta()?.pairedAtMs)
            repo.setPreferredProfile(null)
            assertNull(repo.linkedDevice.value?.preferredProfile)

            repo.forget()
            assertNull(repo.linkedDevice.value)
            assertNull(vault.meta())
            assertFalse(vault.hasPairing())
        } finally {
            base.deleteRecursively() // temp dir created above by createTempDirectory
        }
    }
}
