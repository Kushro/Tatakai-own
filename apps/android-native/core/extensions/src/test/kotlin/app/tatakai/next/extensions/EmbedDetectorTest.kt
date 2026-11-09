// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Container
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave B (16 §5 t.6c, worker t): pure detection logic of the embed sniffer.
class EmbedDetectorTest {
    @Test
    fun detectsMediaByPathQueryAndContentType() {
        assertEquals(Container.HLS, EmbedDetector.mediaContainer("https://cdn.example/a/master.m3u8?token=1"))
        assertEquals(Container.MP4, EmbedDetector.mediaContainer("https://cdn.example/v.mp4#t=10"))
        assertEquals(Container.HLS, EmbedDetector.mediaContainer("https://cdn.example/file.php?src=x.m3u8"))
        assertEquals(Container.HLS, EmbedDetector.mediaContainer("https://cdn.example/stream", "application/vnd.apple.mpegurl; charset=utf-8"))
        assertEquals(Container.MP4, EmbedDetector.mediaContainer("https://cdn.example/stream", "video/mp4"))
        assertNull(EmbedDetector.mediaContainer("https://cdn.example/manifest.mpd"))
        assertNull(EmbedDetector.mediaContainer("https://cdn.example/page.html", "text/html"))
        assertNull(EmbedDetector.mediaContainer("blob:https://cdn.example/1"))
    }

    @Test
    fun segmentsAndAssetsAreIgnorable() {
        assertTrue(EmbedDetector.isIgnorable("https://cdn.example/seg-001.ts?x=1"))
        assertTrue(EmbedDetector.isIgnorable("https://cdn.example/subs.vtt"))
        assertFalse(EmbedDetector.isIgnorable("https://cdn.example/master.m3u8"))
    }

    @Test
    fun hostListAndParentDomainBlocking() {
        val hosts = EmbedDetector.parseHostList(
            """
            # ads
            doubleclick.net
            0.0.0.0 popads.net
            127.0.0.1   Ads.Example.COM.
            not a host line with spaces
            localhost
            """.trimIndent(),
        )
        assertTrue("doubleclick.net" in hosts)
        assertTrue("popads.net" in hosts)
        assertTrue("ads.example.com" in hosts)
        assertFalse("localhost" in hosts)
        assertTrue(EmbedDetector.isBlockedHost("static.doubleclick.net", hosts))
        assertTrue(EmbedDetector.isBlockedHost("x.ads.example.com", hosts))
        assertFalse(EmbedDetector.isBlockedHost("example.com", hosts))
        assertFalse(EmbedDetector.isBlockedHost("player.example.org", hosts))
    }

    @Test
    fun navigationStaysInsideTheEmbedSite() {
        val embed = "https://player.vidhost.example/e/abc"
        assertTrue(EmbedDetector.navigationAllowed(embed, "https://cdn2.vidhost.example/e/abc?x=1"))
        assertTrue(EmbedDetector.navigationAllowed(embed, "about:blank"))
        assertFalse(EmbedDetector.navigationAllowed(embed, "https://casino.example.net/"))
        assertFalse(EmbedDetector.navigationAllowed(embed, "intent://scan/#Intent;end"))
        assertTrue(EmbedDetector.navigationAllowed(embed, "https://other.example.net/", setOf("example.net")))
    }

    @Test
    fun onlyUsefulHeadersAreReplayed() {
        val h = EmbedDetector.replayHeaders(
            mapOf("referer" to "https://player.vidhost.example/", "Sec-Fetch-Mode" to "cors", "Origin" to "https://player.vidhost.example"),
            cookie = "cf=1",
            userAgent = "UA/1",
        )
        assertEquals(
            mapOf("Referer" to "https://player.vidhost.example/", "Origin" to "https://player.vidhost.example", "Cookie" to "cf=1", "User-Agent" to "UA/1"),
            h,
        )
    }
}
