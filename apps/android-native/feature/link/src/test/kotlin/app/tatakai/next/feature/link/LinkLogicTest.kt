// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.link

import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.LinkHandoff
import app.tatakai.next.model.MediaRef
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue
import kotlin.test.assertNull

/** parity wave B (worker r): hand-off args, clock text and the discovered-PC list of the Linked screen. */
class LinkLogicTest {
    @Test fun handoffOpensTheLinkSessionAtThePcPosition() {
        val args = LinkLogic.handoffArgs(LinkHandoff(sessionId = "s1", animeId = "mal:5", episode = 3, positionSec = 75, title = "T"))
        assertEquals("s1", args.remoteSessionId)
        assertEquals(75_000L, args.startMs)
        assertEquals(MediaRef(malId = 5), args.media)
        assertEquals(3, args.episode)
        assertEquals(MediaRef(anilistId = 21), LinkLogic.refOf("21"))
        assertNull(LinkLogic.refOf("ext:a:b:c"))
    }

    @Test fun theFingerprintFitsOneLine() {
        // BUG-090: 16 groups of 4 (sha-256) took 3 lines of the main card
        val full = "B452 3914 77AA 0C1D 9E2F 3344 5566 7788 99AA BBCC DDEE FF00 1122 3344 0A7C DF6D"
        assertEquals("B452 3914 … 0A7C DF6D", LinkLogic.shortFingerprint(full))
        assertEquals("B452 3914 … 0A7C DF6D", LinkLogic.shortFingerprint(full.replace(' ', ':')))
        assertEquals("AB12 CD34", LinkLogic.shortFingerprint("AB12 CD34"))
    }

    @Test fun enhanceUrlsMustBeCompleteHttpLinks() {
        // BUG-024
        assertTrue(LinkLogic.enhanceUrlValid("https://cdn.example.com/hls/master.m3u8"))
        assertTrue(LinkLogic.enhanceUrlValid("  http://192.168.1.10:8080/a.mp4 "))
        for (bad in listOf("Master.m3u8", "cdn.example.com/master.m3u8", "ftp://h/x.m3u8", "https://", "https://cdn.example.com/a b.m3u8", "Https//x")) {
            assertFalse(LinkLogic.enhanceUrlValid(bad), bad)
        }
        assertTrue(LinkLogic.enhanceRejected(app.tatakai.next.link.LinkApiError.SourceUnavailable))
        assertTrue(LinkLogic.enhanceRejected(app.tatakai.next.link.LinkApiError.UrlSourceDenied))
        assertFalse(LinkLogic.enhanceRejected(null))
    }

    @Test fun clockText() {
        assertEquals("1:05", LinkLogic.clock(65, null))
        assertEquals("12:00 / 24:00", LinkLogic.clock(720, 1440))
        assertEquals("1:00:01 / 1:30:00", LinkLogic.clock(3601, 5400))
    }

    @Test fun pairedPcIsNotOfferedAgain() {
        val a = DiscoveredPc("A", "192.168.1.2", 47321, "id-a")
        val b = DiscoveredPc("B", "192.168.1.3", 47321, "id-b")
        assertEquals(listOf(b), LinkLogic.others(listOf(a, b, b), "192.168.1.2", 47321))
    }
}
