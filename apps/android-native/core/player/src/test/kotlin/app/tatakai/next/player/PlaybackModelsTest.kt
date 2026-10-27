// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class PlaybackModelsTest {
    @Test
    fun fallbackStopsAfterThreeAutomaticMoves() {
        val f = SourceFallback(sourceCount = 5)
        assertEquals(1, f.onFailure())
        assertEquals(2, f.onFailure())
        assertEquals(3, f.onFailure())
        assertNull(f.onFailure(), "the fourth failure is surfaced to the user")
        f.select(0)
        assertEquals(1, f.onFailure(), "a manual choice resets the budget")
    }

    @Test
    fun successfulStartResetsTheBudget() {
        val f = SourceFallback(sourceCount = 3)
        f.onFailure(); f.onFailure()
        f.onPlaybackStarted()
        assertEquals(0, f.fallbacksUsed)
        assertEquals(0, f.onFailure()) // wraps around: 2 -> 0
    }

    @Test
    fun singleSourceNeverFallsBack() {
        assertNull(SourceFallback(1).onFailure())
        assertNull(SourceFallback(0).onFailure())
    }

    @Test
    fun historyKeysFollowE9() {
        val now = Instant.parse("2026-01-01T00:00:00Z")
        val e = ProgressTracker.entry(21, 5114, "Title", null, 7, positionMs = 1_300_000, durationMs = 1_400_000, now = now)!!
        assertEquals("21", e.animeId)
        assertEquals("21?ep=7", e.episodeId)
        assertEquals(1300, e.progressSec)
        assertEquals(1400, e.durationSec)
        assertTrue(e.completed)
        assertEquals(21, e.anilistId)
        assertEquals(5114, e.malId)

        val mal = ProgressTracker.entry(null, 5114, "T", "p", 1, 10_000, 1_400_000, now)!!
        assertEquals("mal:5114", mal.animeId)
        assertEquals("mal:5114?ep=1", mal.episodeId)
        assertFalse(mal.completed)
        assertNull(mal.anilistId)

        assertNull(ProgressTracker.entry(null, null, "T", null, 1, 0, 0, now))
        assertNull(ProgressTracker.entry(21, null, "T", null, 1, 0, 0, now)!!.durationSec)
    }
}
