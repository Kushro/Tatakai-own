// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import java.io.ByteArrayOutputStream
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class ServerPackageTest {
    private val manifest = """{"id":"tatakai.extension.toko","name":"Toko","version":"4.6.0","type":"custom","description":"d","permissions":["network:domain:example.com"]}"""
    private val bundle = "module.exports = class { async streamAll() { return [] } };"

    private fun kai(m: String = manifest, b: String = bundle): ByteArray {
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { z ->
            z.putNextEntry(ZipEntry("manifest.json")); z.write(m.toByteArray()); z.closeEntry()
            z.putNextEntry(ZipEntry("bundle.js")); z.write(b.toByteArray()); z.closeEntry()
        }
        return out.toByteArray()
    }

    private val sha = sha256Hex(bundle.toByteArray())

    @Test
    fun aPackageMatchingTheAnnouncedHashInstalls() {
        // BUG-055: Toko 4.6.0 from the PC's extension server.
        assertNull(verifyServerPackage(ServerPackage(kai(), sha.uppercase()), "tatakai.extension.toko", sha))
        assertNull(verifyServerPackage(ServerPackage(kai(), sha), "tatakai.extension.toko", listedSha256 = null))
    }

    @Test
    fun aTamperedOrUnannouncedBundleIsRefused() {
        val other = kai(b = "$bundle // changed")
        assertEquals(PackageProblem.HASH_MISMATCH, verifyServerPackage(ServerPackage(other, sha), "tatakai.extension.toko", null))
        assertEquals(PackageProblem.HASH_MISMATCH, verifyServerPackage(ServerPackage(kai(), null), "tatakai.extension.toko", sha))
        assertEquals(PackageProblem.HASH_MISMATCH, verifyServerPackage(ServerPackage(kai(), sha), "tatakai.extension.toko", "0".repeat(64)))
    }

    @Test
    fun anotherExtensionOrGarbageIsRefused() {
        assertEquals(PackageProblem.WRONG_EXTENSION, verifyServerPackage(ServerPackage(kai(), sha), "tatakai.extension.latino", null))
        assertEquals(PackageProblem.UNREADABLE, verifyServerPackage(ServerPackage("not a zip".toByteArray(), sha), "tatakai.extension.toko", null))
    }
}
