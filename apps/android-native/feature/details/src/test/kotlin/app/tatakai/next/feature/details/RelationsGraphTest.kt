// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class RelationsGraphTest {
    private fun m(id: Int) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "T$id", titles = emptyMap(), cover = null, banner = null,
        color = null, synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null,
        sourceProvider = "anilist",
    )

    @Test
    fun chainFromPrequelAndSequelWithOthersBelowTheOpenTitle() {
        val rels = listOf(
            MediaRelation("SIDE_STORY", m(10)),
            MediaRelation("SEQUEL", m(3)),
            MediaRelation("PREQUEL", m(1)),
            MediaRelation("ADAPTATION", m(20)),
        )
        val g = layoutRelationsGraph(m(2), seasons = emptyList(), relations = rels)
        val chain = g.nodes.filter { it.row == 0 }.sortedBy { it.col }
        assertEquals(listOf(1, 2, 3), chain.map { it.media.ref.anilistId })
        assertTrue(chain[1].current)
        assertEquals("PREQUEL", chain[0].relation)
        val others = g.nodes.filter { it.row == 1 }
        // Classic order: side story before adaptation; two nodes centred under column 1 -> columns 1 and 2.
        assertEquals(listOf(10, 20), others.sortedBy { it.col }.map { it.media.ref.anilistId })
        assertEquals(listOf(1, 2), others.map { it.col }.sorted())
        assertEquals(2, g.edges.count { it.main })
        assertEquals(2, g.edges.count { !it.main && it.from == "a2" })
        assertEquals(3, g.cols)
        assertEquals(2, g.rows)
    }

    @Test
    fun watchOrderIsTheChainAndOthersWrap() {
        val seasons = listOf(m(1), m(2), m(3), m(4))
        val rels = (10..15).map { MediaRelation("SPIN_OFF", m(it)) } + MediaRelation("SEQUEL", m(3))
        val g = layoutRelationsGraph(m(3), seasons, rels, perRow = 4)
        assertEquals(listOf(1, 2, 3, 4), g.nodes.filter { it.row == 0 }.sortedBy { it.col }.map { it.media.ref.anilistId })
        assertEquals(2, g.nodes.single { it.current }.col)
        // The sequel already in the chain is not repeated; six spin-offs = a row of four and a row of two.
        assertEquals(6, g.nodes.count { it.row > 0 })
        assertEquals(4, g.nodes.count { it.row == 1 })
        assertEquals(2, g.nodes.count { it.row == 2 })
        assertEquals(g.nodes.size, g.nodes.map { it.key }.toSet().size)
    }

    @Test
    fun aTitleAloneIsOneNode() {
        val g = layoutRelationsGraph(m(5), emptyList(), emptyList())
        assertEquals(1, g.nodes.size)
        assertTrue(g.edges.isEmpty())
    }

    @Test
    fun ratingLabelRoundsToOneDecimal() {
        assertEquals("8.0", ratingLabel(8.03, java.util.Locale.ROOT))
        assertEquals(null, ratingLabel(null))
        assertEquals(null, ratingLabel(0.0))
    }
}
