// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import java.net.URLEncoder
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DeepLinkTest {
    // Same shape as core.mjs: code = two random UUIDs, server = encodeURIComponent(origin).
    private val code = "0f8fad5b-d9cb-469f-a165-70867728950e7c9e6679-7425-40de-944b-e07fc1f90ae7"

    private fun enc(s: String) = URLEncoder.encode(s, "UTF-8").replace("+", "%20")

    private fun link(server: String, c: String = code) = "tatakai://extensionserver/connect?server=${enc(server)}&code=${enc(c)}"

    @Test
    fun parsesTheCanonicalLink() {
        val parsed = DeepLink.parse(link("http://192.168.1.20:8787"))
        assertEquals(PairingLink("http://192.168.1.20:8787", code), parsed)
        assertEquals(PairingLink("https://pc.example.org", code), DeepLink.parse(link("https://pc.example.org")))
        assertEquals(PairingLink("http://[fd7a:115c:a1e0::5]:8787", code), DeepLink.parse(link("http://[fd7a:115c:a1e0::5]:8787")))
    }

    @Test
    fun acceptsCaseTrailingSlashSpacesAndParamOrder() {
        assertEquals(
            PairingLink("http://192.168.1.20:8787", "abc_DEF-123"),
            DeepLink.parse("  TATAKAI://ExtensionServer/connect/?code=abc_DEF-123&server=http%3A%2F%2F192.168.1.20%3A8787&extra=1  "),
        )
    }

    @Test
    fun roundTripsThroughBuild() {
        val l = PairingLink("http://10.0.0.2:8787", code)
        assertEquals(l, DeepLink.parse(DeepLink.build(l)))
    }

    @Test
    fun rejectsOtherLinksAndMalformedValues() {
        listOf(
            "", "https://example.org/connect-server?server=x&code=y", "tatakai://extensionserver/other?server=x&code=y",
            "tatakai://other/connect?server=x&code=y", "tatakai-link:v1?h=192.168.1.2&p=1",
            "tatakai://extensionserver/connect", "tatakai://extensionserver/connect?server=${enc("http://192.168.1.2")}",
            "tatakai://extensionserver/connect?code=$code", link("http://192.168.1.2/path"), link("http://192.168.1.2?x=1"),
            link("ftp://192.168.1.2"), link("http://192.168.1.2", "bad code!"), link("http://192.168.1.2", "x".repeat(129)),
            link("http://192.168.1.2", ""), link("http://user:pw@192.168.1.2"),
            "tatakai://extensionserver/connect?server=a&server=b&code=$code",
            "tatakai://extensionserver:99/connect?server=${enc("http://192.168.1.2")}&code=$code",
        ).forEach { assertNull(DeepLink.parse(it), it) }
    }

    @Test
    fun publicHttpServerIsReportedAsInsecureHost() {
        val r = DeepLink.parseResult(link("http://example.org:8787"))
        val insecure = assertIs<DeepLinkResult.InsecureHost>(r)
        assertEquals("example.org", insecure.host)
        assertIs<DeepLinkResult.Invalid>(DeepLink.parseResult("garbage"))
    }

    @Test
    fun toStringNeverShowsTheCode() {
        val text = PairingLink("http://192.168.1.2", code).toString()
        assertFalse(code in text)
        assertTrue("192.168.1.2" in text)
    }
}
