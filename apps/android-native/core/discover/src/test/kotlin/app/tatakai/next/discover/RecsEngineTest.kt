// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.RecReasonKind
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class RecsEngineTest {
    private val history = listOf(
        RecsEngine.Signal(media(1, listOf("Action", "Fantasy"), 2019, 85), affinity = 1f, userScore = 90, studios = listOf("MAPPA")),
        RecsEngine.Signal(media(2, listOf("Fantasy", "Adventure"), 2021, 80), affinity = 0.85f, studios = listOf("MAPPA")),
        RecsEngine.Signal(media(3, listOf("Romance"), 2005, 60), affinity = -0.4f, userScore = 40),
    )

    @Test
    fun affinityFollowsClassicRules() {
        assertEquals(1f, RecsEngine.affinityOf(ListStatus.COMPLETED, null, completed = true))
        assertTrue(RecsEngine.affinityOf(ListStatus.DROPPED, 20, completed = false) < 0f)
        assertEquals(0.4f, RecsEngine.affinityOf(null, null, completed = false))
        assertEquals(0.6f, RecsEngine.affinityOf(null, null, completed = true))
        assertTrue(RecsEngine.affinityOf(ListStatus.PLANNING, 100, completed = false) > RecsEngine.affinityOf(ListStatus.PLANNING, null, completed = false))
    }

    @Test
    fun profileOnlyFromPositiveSignals() {
        val p = RecsEngine.profile(history)!!
        assertEquals(2, p.sampleSize)
        assertEquals(1f, p.genres["Fantasy"])
        assertNull(p.genres["Romance"], "dropped titles do not shape taste")
        assertEquals(1f, p.studios["MAPPA"])
        assertEquals(setOf(2010, 2020), p.eras.keys)
        assertEquals(65, p.preferredScore, "mean of the user's own scores")
        assertNull(RecsEngine.profile(listOf(history[2])))
    }

    @Test
    fun rankPrefersAffineTitlesAndExplainsThem() {
        val p = RecsEngine.profile(history)!!
        val fantasy = RecsEngine.Candidate(media(10, listOf("Fantasy", "Action"), 2022, 84), studios = listOf("MAPPA"), similarTo = "T1")
        val romance = RecsEngine.Candidate(media(11, listOf("Romance", "Slice of Life"), 1995, 50))
        val seen = RecsEngine.Candidate(media(1))
        val ranked = RecsEngine.rank(listOf(romance, fantasy, seen), p, seen = setOf("1"), dismissed = emptySet(), limit = 10, currentYear = 2026)
        assertEquals(listOf(10, 11), ranked.map { it.media.ref.anilistId })
        assertTrue(ranked[0].affinity > ranked[1].affinity)
        assertTrue(ranked[0].affinity in 0f..1f)
        val kinds = ranked[0].reasons.map { it.kind }
        assertTrue(RecReasonKind.SIMILAR_TO in kinds && RecReasonKind.GENRE in kinds, "reasons: $kinds")
        assertEquals("T1", ranked[0].reasons.first { it.kind == RecReasonKind.SIMILAR_TO }.label)
    }

    @Test
    fun dismissedAndDuplicatesAreDropped() {
        val p = RecsEngine.profile(history)!!
        val a = RecsEngine.Candidate(media(20, listOf("Fantasy")))
        val a2 = RecsEngine.Candidate(media(20, listOf("Fantasy")), similarTo = "T2")
        val b = RecsEngine.Candidate(media(21, listOf("Fantasy")))
        val ranked = RecsEngine.rank(listOf(a, a2, b), p, emptySet(), dismissed = setOf("21"), limit = 5, currentYear = 2026)
        assertEquals(listOf(20), ranked.map { it.media.ref.anilistId })
        assertTrue(ranked.single().reasons.any { it.kind == RecReasonKind.SIMILAR_TO }, "duplicates merge their facts")
    }

    @Test
    fun atlasIsDeterministicAndNormalised() {
        val seen = listOf(media(1, listOf("Action", "Fantasy")), media(2, listOf("Fantasy")), media(3, listOf("Romance", "Drama")))
        val recs = listOf(media(4, listOf("Romance")), media(5, listOf("Action")))
        val a = RecsEngine.atlas(seen, recs, clusters = 2)
        val b = RecsEngine.atlas(seen, recs, clusters = 2)
        assertEquals(a, b)
        assertEquals(5, a.size)
        assertTrue(a.all { it.x in 0f..1f && it.y in 0f..1f })
        assertEquals(listOf(false, false, false, true, true), a.map { it.recommended })
        assertTrue(a.map { it.cluster }.toSet().size == 2)
        val romance = a.first { it.media.ref.anilistId == 3 }.cluster
        assertEquals(romance, a.first { it.media.ref.anilistId == 4 }.cluster, "similar genres share a cluster")
    }

    @Test
    fun emptyAtlas() {
        assertTrue(RecsEngine.atlas(emptyList(), emptyList()).isEmpty())
    }
}
