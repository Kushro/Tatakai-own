// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.link

import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.LinkHandoff
import app.tatakai.next.model.MediaRef
import kotlin.test.Test
import kotlin.test.assertEquals
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
