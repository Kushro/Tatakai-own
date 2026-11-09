// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.policy

import app.tatakai.next.model.torrent.BandwidthSchedule
import app.tatakai.next.model.torrent.EpisodeAlignment
import app.tatakai.next.model.torrent.TorrentSettings
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** parity wave A (worker q): bandwidth schedule, cache limit, episode alignment, next-episode preload. */
class ParityPoliciesTest {

    @Test
    fun scheduleWindowsIncludingMidnight() {
        val day = BandwidthSchedule(startMinute = 8 * 60, endMinute = 23 * 60)
        assertTrue(day.contains(8 * 60))
        assertTrue(day.contains(22 * 60 + 59))
        assertFalse(day.contains(23 * 60))
        assertFalse(day.contains(3 * 60))
        val night = BandwidthSchedule(startMinute = 23 * 60, endMinute = 7 * 60)
        assertTrue(night.contains(23 * 60 + 30))
        assertTrue(night.contains(0))
        assertTrue(night.contains(6 * 60 + 59))
        assertFalse(night.contains(7 * 60))
        assertFalse(night.contains(12 * 60))
        assertFalse(BandwidthSchedule(600, 600).contains(600), "an empty window never matches")
    }

    @Test
    fun scheduleSwapsTheLimitsOnlyInsideTheWindow() {
        val s = TorrentSettings(downloadLimitKib = 0, uploadLimitKib = 50, schedule = BandwidthSchedule(8 * 60, 23 * 60, downloadLimitKib = 1_024, uploadLimitKib = 0))
        val inside = applySchedule(s, 12 * 60)
        assertEquals(1_024, inside.downloadLimitKib)
        assertEquals(0, inside.uploadLimitKib)
        assertEquals(s, applySchedule(s, 2 * 60))
        assertEquals(TorrentSettings(), applySchedule(TorrentSettings(), 12 * 60))
    }

    @Test
    fun cacheEvictsOldestUnusedUntilUnderTheLimit() {
        val items = listOf(
            CacheItem("new", addedUnix = 300, bytes = 400, inUse = false),
            CacheItem("old", addedUnix = 100, bytes = 500, inUse = false),
            CacheItem("playing", addedUnix = 50, bytes = 600, inUse = true),
            CacheItem("mid", addedUnix = 200, bytes = 300, inUse = false),
        )
        assertEquals(listOf("old", "mid"), cacheEvictions(items, limitBytes = 1_000))
        assertEquals(emptyList(), cacheEvictions(items, limitBytes = 0))
        assertEquals(emptyList(), cacheEvictions(items, limitBytes = 10_000))
        assertEquals(listOf("old", "mid", "new"), cacheEvictions(items, limitBytes = 1))
    }

    private val pack = listOf(
        AlignFile(0, "Show/Show - 03.mkv", 3),
        AlignFile(1, "Show/Show - 01.mkv", 1),
        AlignFile(2, "Show/Show - 02.mkv", 2),
        AlignFile(3, "Show/Extra.mkv", null),
    )

    @Test
    fun alignmentKeepsParsedNumbersPlusOffsetAndNumbersTheRestByOrder() {
        assertEquals(mapOf(3 to 1, 1 to 1, 2 to 2, 0 to 3), alignedEpisodes(pack, EpisodeAlignment(firstEpisode = 1, offset = 0)))
        val shifted = alignedEpisodes(pack, EpisodeAlignment(firstEpisode = 13, offset = 12))
        assertEquals(13, shifted[1])
        assertEquals(15, shifted[0])
        assertEquals(13, shifted[3])
        assertEquals(2, alignedIndex(pack, EpisodeAlignment(1, 0), 2))
        assertNull(alignedIndex(pack, EpisodeAlignment(1, 0), 9))
        assertTrue(alignedEpisodes(pack, EpisodeAlignment(1, -5)).values.all { it >= 1 }, "episodes below 1 are dropped")
    }

    @Test
    fun preloadPicksTheNextEpisodeThenTheNextFile() {
        assertEquals(2, preloadTarget(pack, current = 1))
        assertEquals(0, preloadTarget(pack, current = 2))
        val unnumbered = listOf(AlignFile(5, "a.mkv", null), AlignFile(6, "b.mkv", null))
        assertEquals(6, preloadTarget(unnumbered, 5))
        assertNull(preloadTarget(unnumbered, 6))
        assertNull(preloadTarget(unnumbered, 42))
    }
}
