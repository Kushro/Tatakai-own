// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import kotlinx.coroutines.runBlocking
import kotlin.test.Test
import kotlin.test.assertEquals

class WatchOrderTest {
    private fun m(id: Int, kind: MediaKind = MediaKind.ANIME) = Media(
        MediaRef(anilistId = id), kind, "T$id", emptyMap(), null, null, null, null, emptyList(), null, null, null, null, null, "anilist",
    )

    /** 1 <- 2 <- 3 (self) -> 4 -> 5, plus a manga source and a side story that must not enter the chain. */
    private val graph: Map<Int, List<MediaRelation>> = mapOf(
        1 to listOf(MediaRelation("SEQUEL", m(2))),
        2 to listOf(MediaRelation("PREQUEL", m(1)), MediaRelation("SEQUEL", m(3))),
        3 to listOf(
            MediaRelation("SOURCE", m(100, MediaKind.MANGA)),
            MediaRelation("PREQUEL", m(2)),
            MediaRelation("SIDE_STORY", m(50)),
            MediaRelation("SEQUEL", m(4)),
        ),
        4 to listOf(MediaRelation("PREQUEL", m(3)), MediaRelation("SEQUEL", m(5))),
        5 to listOf(MediaRelation("PREQUEL", m(4)), MediaRelation("SEQUEL", m(3))), // a loop back must not repeat 3
    )

    @Test
    fun walksPrequelsThenSequels() = runBlocking {
        val hops = mutableListOf<Int>()
        val order = WatchOrder.walk(m(3), graph.getValue(3)) { id -> hops += id; graph[id].orEmpty() }
        assertEquals(listOf(1, 2, 3, 4, 5), order.map { it.ref.anilistId })
        assertEquals(listOf(2, 1, 4, 5), hops)
    }

    @Test
    fun aStandaloneTitleIsItsOwnOrder() = runBlocking {
        assertEquals(listOf(9), WatchOrder.walk(m(9), emptyList()) { emptyList() }.map { it.ref.anilistId })
    }

    @Test
    fun theWalkIsBounded() = runBlocking {
        // An endless prequel chain stops after MAX_HOPS.
        var calls = 0
        val order = WatchOrder.walk(m(1000), listOf(MediaRelation("PREQUEL", m(999)))) { id ->
            calls++
            listOf(MediaRelation("PREQUEL", m(id - 1)))
        }
        assertEquals(WatchOrder.MAX_HOPS, calls)
        assertEquals(WatchOrder.MAX_HOPS + 1, order.size)
    }
}
