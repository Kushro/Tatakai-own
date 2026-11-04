// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** Same cases as the tests of apps/desktop-native/crates/tk-ext/src/browser.rs, plus the manifest parsing of `browserDomains`. */
class BrowserPolicyTest {
    private fun manifest(browser: List<String>, perms: List<String>): ExtensionManifest {
        fun arr(l: List<String>) = l.joinToString(",", "[", "]") { "\"$it\"" }
        return ManifestParser.parse(
            """{"id":"latino","name":"Latino","version":"1.0.0","type":"onlinestream","description":"",""" +
                """"permissions":${arr(perms)},"browserDomains":${arr(browser)}}""",
        )
    }

    @Test
    fun browserDomainsNeedANetworkPermission() {
        val m = manifest(
            listOf("sololatino.net", "www2.pelisforte.se", "evil.example", "127.0.0.1", "SoloLatino.net", "x.local"),
            listOf("network:domain:sololatino.net", "network:domain:pelisforte.se", "network:domain:127.0.0.1"),
        )
        assertEquals(listOf("sololatino.net", "www2.pelisforte.se"), BrowserPolicy.browserDomains(m))
        assertTrue(BrowserPolicy.browserDomains(manifest(emptyList(), listOf("network:domain:a.com"))).isEmpty())
    }

    @Test
    fun wildcardPermissionsAndInvalidDomains() {
        val m = manifest(
            listOf("cdn.toko.example", "localhost", "a.localhost", "bad_domain.example", "toko.example", " TOKO.example "),
            listOf("network:domain:*.toko.example", "network:domain:*"),
        )
        // "*." is stripped from the permission; a bare "*" grants no browser domain; dedup after trim + lower-case
        assertEquals(listOf("cdn.toko.example", "toko.example"), BrowserPolicy.browserDomains(m))
    }

    @Test
    fun urlsAreCheckedLikeTheLegacyHost() {
        val d = listOf("sololatino.net")
        assertTrue(BrowserPolicy.urlAllowed("https://sololatino.net/anime/x", d))
        assertTrue(BrowserPolicy.urlAllowed("https://www.sololatino.net/", d))
        assertFalse(BrowserPolicy.urlAllowed("http://sololatino.net/", d))
        assertFalse(BrowserPolicy.urlAllowed("https://sololatino.net:8443/", d))
        assertFalse(BrowserPolicy.urlAllowed("https://user@sololatino.net/", d))
        assertFalse(BrowserPolicy.urlAllowed("https://notsololatino.net/", d))
        assertFalse(BrowserPolicy.urlAllowed("https://challenges.cloudflare.com/x", d))
        assertTrue(BrowserPolicy.subresourceAllowed("https://challenges.cloudflare.com/x", d))
    }

    @Test
    fun urlEdgeCases() {
        val d = listOf("sololatino.net")
        assertTrue(BrowserPolicy.urlAllowed("HTTPS://SoloLatino.NET/a?b=c|d e", d))
        assertTrue(BrowserPolicy.urlAllowed("https://sololatino.net:443/", d)) // default port = no port (WHATWG)
        assertFalse(BrowserPolicy.urlAllowed("https://evil.example\\@sololatino.net/", d))
        assertFalse(BrowserPolicy.urlAllowed("https://evil.example#@sololatino.net", d))
        assertFalse(BrowserPolicy.urlAllowed("https://user:pw@sololatino.net/", d))
        assertFalse(BrowserPolicy.urlAllowed("javascript:alert(1)", d))
        assertFalse(BrowserPolicy.urlAllowed("not a url", d))
        assertFalse(BrowserPolicy.urlAllowed("https:///", d))
        assertFalse(BrowserPolicy.subresourceAllowed("https://evil.example/", d))
    }

    @Test
    fun operationTimeoutClampsLikeLegacy() {
        assertEquals(20_000, browserOperationTimeoutMs(buildJsonObject { }))
        assertEquals(15_000, browserOperationTimeoutMs(buildJsonObject { put("timeout", 15) }))
        assertEquals(30_000, browserOperationTimeoutMs(buildJsonObject { put("timeout", 45) }))
        assertEquals(1_000, browserOperationTimeoutMs(buildJsonObject { put("timeout", 0.2) }))
        assertEquals(20_000, browserOperationTimeoutMs(buildJsonObject { put("timeout", 0) }))
        assertEquals(12_000, browserOperationTimeoutMs(buildJsonObject { put("timeout", " 12 ") }))
        assertEquals(20_000, browserOperationTimeoutMs(buildJsonObject { put("timeout", "soon") }))
        assertTrue(browserWantsWindow(buildJsonObject { put("headless", false) }))
        assertFalse(browserWantsWindow(buildJsonObject { put("headless", JsonPrimitive("false")) }))
        assertFalse(browserWantsWindow(buildJsonObject { put("headless", true) }))
    }
}
