// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.io.ByteArrayOutputStream
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

class ManifestAndCompatTest {
    private val valid = """{"id":"demo.src","name":"Demo","version":"1.0.0","type":"onlinestream","description":"d","permissions":["network:domain:example.com"]}"""

    @Test
    fun validManifestParses() {
        val m = ManifestParser.parse(valid)
        assertEquals("demo.src", m.id)
        assertEquals(ExtensionType.ONLINESTREAM, m.type)
        assertEquals("demo.src", m.namespace)
        assertEquals(listOf("sources", "stream", "torrent"), m.enabledAggregateRoutes)
        assertEquals(100, m.priority)
    }

    @Test
    fun invalidManifestsNameTheFailingField() {
        fun field(json: String) = assertFailsWith<ManifestException> { ManifestParser.parse(json) }.field
        assertEquals(null, field("not json"))
        assertEquals("id", field(valid.replace("\"id\":\"demo.src\",", "")))
        assertEquals("id", field(valid.replace("demo.src", "Bad Id!")))
        assertEquals("type", field(valid.replace("onlinestream", "widget")))
        assertEquals("permissions", field(valid.replace(",\"permissions\":[\"network:domain:example.com\"]", "")))
        assertEquals("version", field(valid.replace("\"version\":\"1.0.0\"", "\"version\":3")))
        assertEquals(
            "customSources",
            field(valid.dropLast(1) + ""","capabilities":["custom-source"]}"""),
        )
    }

    @Test
    fun networkPolicyMatchesExactAndSubdomainsOnly() {
        val p = NetworkPolicy.fromPermissions(listOf("network:domain:example.com", "parse:html"))
        assertTrue(p.allowsHost("example.com"))
        assertTrue(p.allowsHost("cdn.EXAMPLE.com"))
        assertFalse(p.allowsHost("badexample.com"))
        assertFalse(p.allowsHost("example.com.evil.net"))
        assertTrue(NetworkPolicy.fromPermissions(listOf("network:domain:*")).allowsHost("anything.test"))
        assertFalse(NetworkPolicy.NONE.allowsHost("example.com"))
        assertEquals(ExtError.PermissionDenied.MESSAGE, assertFailsWith<ExtError.PermissionDenied> { p.requireAllowed("https://blocked.test/x") }.message)
        assertFailsWith<ExtError.PermissionDenied> { p.requireAllowed("file:///etc/passwd") }
        assertFailsWith<ExtError.PermissionDenied> { p.requireAllowed("https://user:pw@example.com/") }
    }

    @Test
    fun ssrfGuardRejectsLocalTargets() {
        fun bad(ip: String) = SsrfGuard.isForbidden(java.net.InetAddress.getByName(ip))
        listOf("127.0.0.1", "10.1.2.3", "192.168.0.5", "172.16.9.9", "169.254.169.254", "100.64.0.1", "0.0.0.0", "::1", "fe80::1", "fd00::1", "224.0.0.1")
            .forEach { assertTrue(bad(it), it) }
        listOf("8.8.8.8", "93.184.216.34", "2606:4700:4700::1111").forEach { assertFalse(bad(it), it) }
        assertTrue(SsrfGuard.isForbiddenHostName("LOCALHOST"))
    }

    @Test
    fun compatLevelsFollowTheContract() {
        assertEquals(CompatLevel.FULL, CompatAnalyzer.analyze("const c = require('node:crypto'); require(\"events\");").level)
        assertEquals(CompatLevel.PARTIAL, CompatAnalyzer.analyze("require('http')").level)
        assertEquals(CompatLevel.PARTIAL, CompatAnalyzer.analyze("const n='fs'; require(n)").level)
        assertEquals(CompatLevel.PARTIAL, CompatAnalyzer.analyze("import('x')").level)
        val fs = CompatAnalyzer.analyze("require('fs')")
        assertEquals(CompatLevel.UNSUPPORTED, fs.level)
        assertEquals("Uses Node module 'fs' which the native runtime does not provide", fs.reason)
        assertEquals("Uses module 'cheerio' which the native runtime does not provide", CompatAnalyzer.analyze("require('cheerio')").reason)
    }

    @Test
    fun fixtureSourcesMatchExpectedCompat() {
        val full = CompatAnalyzer.analyze(Fixtures.text("synthetic-full/bundle.js"))
        assertEquals(CompatLevel.FULL, full.level)
        val fs = CompatAnalyzer.analyze(Fixtures.text("synthetic-fs/bundle.js"))
        val expected = Json.parseToJsonElement(Fixtures.text("synthetic-fs/expected.json")).jsonObject
        assertEquals(expected["compat"]!!.jsonPrimitive.content, fs.level.name.lowercase())
        assertEquals(expected["reason"]!!.jsonPrimitive.content, fs.reason)
    }

    @Test
    fun packedFixturesInstallWithExpectedCompat() {
        Fixtures.kai("synthetic-full").inputStream().use { input ->
            val pkg = KaiReader.read(input)
            assertEquals("synthetic-full", pkg.manifest.id)
            assertEquals("synthetic", pkg.manifest.namespace)
            assertEquals(CompatLevel.FULL, pkg.compat.level)
            assertTrue(pkg.enabledByDefault)
            assertEquals(1, pkg.manifest.customSources.size)
        }
        Fixtures.kai("synthetic-fs").inputStream().use { input ->
            val pkg = KaiReader.read(input)
            assertEquals(CompatLevel.UNSUPPORTED, pkg.compat.level)
            assertFalse(pkg.enabledByDefault)
        }
    }

    private fun zip(vararg entries: Pair<String, String>): ByteArray {
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { z ->
            entries.forEach { (n, c) ->
                z.putNextEntry(ZipEntry(n)); z.write(c.toByteArray()); z.closeEntry()
            }
        }
        return out.toByteArray()
    }

    @Test
    fun kaiReaderRejectsBadPackages() {
        val ok = arrayOf("manifest.json" to valid, "bundle.js" to "module.exports={}")
        assertNotNull(KaiReader.read(zip(*ok).inputStream()))
        assertFailsWith<KaiException> { KaiReader.read(zip("manifest.json" to valid).inputStream()) } // no bundle
        assertFailsWith<KaiException> { KaiReader.read(zip(*ok, "../evil.js" to "x").inputStream()) }
        assertFailsWith<KaiException> { KaiReader.read(zip(*ok, "/abs.js" to "x").inputStream()) }
        assertFailsWith<KaiException> { KaiReader.read(zip(*ok, "C:/win.js" to "x").inputStream()) }
        assertFailsWith<KaiException> { KaiReader.read("not a zip".byteInputStream()) }
        val many = (0..70).map { "f$it.txt" to "x" }.toTypedArray()
        assertFailsWith<KaiException> { KaiReader.read(zip(*ok, *many).inputStream()) }
        assertFailsWith<KaiException> {
            KaiReader.read(zip("manifest.json" to valid, "bundle.js" to "a".repeat(KaiReader.MAX_BUNDLE_BYTES + 1)).inputStream())
        }
        assertFailsWith<ManifestException> { KaiReader.read(zip("manifest.json" to "{}", "bundle.js" to "x").inputStream()) }
    }
}
