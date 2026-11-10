// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** List progress (AniList / MAL mirror) joined with the local history: continue watching, resume, watched marks, manual toggle. */
class ListProgressTest {

    private fun h(id: Int, episode: Int, at: Long, progressSec: Int = 300, duration: Int? = 1400, completed: Boolean = false) = HistoryEntry(
        animeId = id.toString(), title = "H$id", poster = null, episodeId = HistoryEntry.episodeIdOf(id.toString(), episode), episode = episode,
        progressSec = progressSec, durationSec = duration, completed = completed, watchedAt = Instant.ofEpochMilli(at), anilistId = id, malId = null,
    )

    private fun l(id: Int, progress: Int, total: Int?, at: Long, status: ListStatus = ListStatus.WATCHING) = ListEntry(
        ref = MediaRef(anilistId = id), title = "L$id", poster = "p$id", status = status, progress = progress, totalEpisodes = total, updatedAtMs = at,
    )

    // ---------------------------------------------------------------- continue watching

    @Test
    fun listOnlyTitleShowsTheNextEpisodeWithoutPosition() {
        // "Even the Student Council Has Its Holes!": watching 2/12 on AniList, never played here
        val out = ListProgress.mergeContinueWatching(emptyList(), listOf(l(10, 2, 12, at = 5_000)), limit = 20)
        val item = out.single()
        assertEquals(3, item.episode)
        assertEquals("10?ep=3", item.episodeId)
        assertEquals(0, item.progressSec)
        assertNull(item.durationSec, "no time bar for a list-only item")
        assertFalse(item.completed)
        assertEquals(10, item.anilistId)
        assertEquals("L10", item.title)
        assertEquals(Instant.ofEpochMilli(5_000), item.watchedAt)
    }

    @Test
    fun oneItemPerAnimeWithTheFurthestEpisode() {
        // history at ep 2 (partial), list says 4 watched: ep 5 without the old position
        val ahead = ListProgress.mergeContinueWatching(listOf(h(1, 2, at = 1_000)), listOf(l(1, 4, 12, at = 900)), limit = 20).single()
        assertEquals(5, ahead.episode)
        assertEquals(0, ahead.progressSec)
        assertNull(ahead.durationSec)
        assertEquals(Instant.ofEpochMilli(1_000), ahead.watchedAt, "ordered by the newer of the two dates")

        // history at ep 3 (partial), list says 2: the history row with its position wins
        val behind = ListProgress.mergeContinueWatching(listOf(h(1, 3, at = 1_000, progressSec = 640)), listOf(l(1, 2, 12, at = 2_000)), limit = 20).single()
        assertEquals(3, behind.episode)
        assertEquals(640, behind.progressSec)
        assertEquals(1400, behind.durationSec)
        assertEquals(Instant.ofEpochMilli(2_000), behind.watchedAt)
    }

    @Test
    fun finishedOrInactiveListEntriesAreNotAdded() {
        val list = listOf(
            l(1, 12, 12, at = 1),
            l(2, 3, 12, at = 2, status = ListStatus.PAUSED),
            l(3, 0, null, at = 3, status = ListStatus.PLANNING),
            l(4, 5, null, at = 4, status = ListStatus.REPEATING),
            l(5, 7, 0, at = 5),
        )
        val out = ListProgress.mergeContinueWatching(emptyList(), list, limit = 20)
        assertEquals(listOf(5, 4), out.map { it.anilistId }, "P >= T, paused and planning are out; unknown totals stay")
        assertEquals(listOf(8, 6), out.map { it.episode })
        // a history row of a finished list entry stays as today
        val kept = ListProgress.mergeContinueWatching(listOf(h(1, 11, at = 9)), list.take(1), limit = 20).single()
        assertEquals(11, kept.episode)
        assertEquals(300, kept.progressSec)
    }

    @Test
    fun newestFirstAndLimited() {
        val history = listOf(h(1, 2, at = 3_000), h(2, 5, at = 1_000))
        val list = listOf(l(3, 1, 12, at = 4_000), l(2, 4, 24, at = 500), l(4, 2, 12, at = 2_000))
        val out = ListProgress.mergeContinueWatching(history, list, limit = 3)
        assertEquals(listOf(3, 1, 4), out.map { it.anilistId })
        assertEquals(4, ListProgress.mergeContinueWatching(history, list, limit = 20).size, "anime 2 appears once")
    }

    @Test
    fun unstartedListEntriesAreLeftOut() {
        // ~340 "watching" entries at 0: only started ones join; a history row of an unstarted entry still shows
        val list = listOf(l(1, 0, 12, at = 9_000), l(2, 0, null, at = 8_000), l(3, 1, 12, at = 1_000))
        val out = ListProgress.mergeContinueWatching(listOf(h(2, 1, at = 500)), list, limit = 20)
        assertEquals(listOf(2, 3), out.map { it.anilistId })
        assertEquals(listOf(1, 2), out.map { it.episode })
        assertEquals(1, ListProgress.mergeContinueReading(emptyList(), listOf(r(1, 0, 20, at = 9), r(2, 3, 20, at = 1)), limit = 10).size)
        val reading = ListProgress.mergeContinueReading(listOf(pos(1, 2.0, completed = false, at = 5)), listOf(r(1, 0, 20, at = 9)), limit = 10).single()
        assertTrue(reading.resume, "the saved page of an unstarted entry still shows")
    }

    @Test
    fun malOnlyEntriesMatchByMalId() {
        val history = h(0, 1, at = 1).copy(animeId = "mal:77", episodeId = "mal:77?ep=1", anilistId = null, malId = 77)
        val list = ListEntry(MediaRef(malId = 77), "M", null, ListStatus.WATCHING, progress = 3, totalEpisodes = 10, updatedAtMs = 2)
        val out = ListProgress.mergeContinueWatching(listOf(history), listOf(list), limit = 20).single()
        assertEquals("mal:77?ep=4", out.episodeId)
    }

    // ---------------------------------------------------------------- details resume

    @Test
    fun resumeEpisodeTakesTheListIntoAccount() {
        assertEquals(1, ListProgress.resumeEpisode(historyTarget = 1, progress = 0, total = 12))
        assertEquals(3, ListProgress.resumeEpisode(historyTarget = 1, progress = 2, total = 12), "2/12 on the list -> episode 3, not 1")
        assertEquals(6, ListProgress.resumeEpisode(historyTarget = 6, progress = 2, total = 12), "history ahead wins")
        assertEquals(12, ListProgress.resumeEpisode(historyTarget = 13, progress = 11, total = 12), "capped to the total")
        assertEquals(12, ListProgress.resumeEpisode(historyTarget = 12, progress = 12, total = 12), "finished: today's behaviour")
        assertEquals(1, ListProgress.resumeEpisode(historyTarget = 1, progress = 12, total = 12), "finished: today's behaviour")
        assertEquals(41, ListProgress.resumeEpisode(historyTarget = 3, progress = 40, total = null), "unknown total: no cap")
    }

    // ---------------------------------------------------------------- watched marks and toggle

    @Test
    fun episodesUpToTheListProgressAreWatched() {
        assertTrue(ListProgress.isEpisodeWatched(1, listProgress = 2, history = null))
        assertTrue(ListProgress.isEpisodeWatched(2, listProgress = 2, history = h(1, 2, at = 1, completed = false)))
        assertFalse(ListProgress.isEpisodeWatched(3, listProgress = 2, history = h(1, 3, at = 1, completed = false)))
        assertTrue(ListProgress.isEpisodeWatched(3, listProgress = 2, history = h(1, 3, at = 1, completed = true)))
        assertFalse(ListProgress.isEpisodeWatched(0, listProgress = 2, history = null))
    }

    @Test
    fun toggleProgressMath() {
        assertEquals(5, ListProgress.toggledProgress(current = 2, episode = 5, watched = true))
        assertEquals(4, ListProgress.toggledProgress(current = 4, episode = 2, watched = true), "never goes back when marking")
        assertEquals(12, ListProgress.toggledProgress(current = 2, episode = 14, watched = true, total = 12))
        assertEquals(2, ListProgress.toggledProgress(current = 4, episode = 3, watched = false))
        assertEquals(1, ListProgress.toggledProgress(current = 1, episode = 3, watched = false), "already below: unchanged")
        assertEquals(0, ListProgress.toggledProgress(current = 4, episode = 1, watched = false))
        assertEquals(0, ListProgress.toggledProgress(current = 0, episode = 0, watched = false))
    }

    @Test
    fun unmarkingACompletedEntryReopensIt() {
        assertEquals(ListStatus.WATCHING, ListProgress.statusAfterUnmark(ListStatus.COMPLETED))
        assertNull(ListProgress.statusAfterUnmark(ListStatus.WATCHING))
        assertNull(ListProgress.statusAfterUnmark(ListStatus.PAUSED))
        assertNull(ListProgress.statusAfterUnmark(null))
    }

    // ---------------------------------------------------------------- read lists (chapters)

    private fun pos(id: Int, chapter: Double?, completed: Boolean, at: Long) =
        ReadPosition(id.toString(), MediaRef(anilistId = id), "P$id", null, chapter, completed, at)

    private fun r(id: Int, progress: Int, total: Int?, at: Long, status: ListStatus = ListStatus.WATCHING) =
        ReadEntry(MediaRef(anilistId = id), "R$id", "c$id", status, progress, total, updatedAtMs = at)

    @Test
    fun chapterMarksFollowTheListProgress() {
        assertTrue(ListProgress.isChapterRead(10.0, 10))
        assertTrue(ListProgress.isChapterRead(10.5, 10), "10.5 belongs to chapter 10")
        assertFalse(ListProgress.isChapterRead(11.0, 10))
        assertFalse(ListProgress.isChapterRead(null, 10))
        assertEquals(10, ListProgress.chapterIndex(10.5))
    }

    @Test
    fun savedChapterOnlyWinsWhenNotBehindTheList() {
        assertTrue(ListProgress.resumeSaved(savedChapter = 3.0, savedCompleted = false, listProgress = 2))
        assertFalse(ListProgress.resumeSaved(savedChapter = 2.0, savedCompleted = false, listProgress = 5))
        assertFalse(ListProgress.resumeSaved(savedChapter = 7.0, savedCompleted = true, listProgress = 2))
        assertTrue(ListProgress.resumeSaved(savedChapter = null, savedCompleted = false, listProgress = 0))
        assertEquals(5.0, ListProgress.readThrough(savedChapter = 2.0, listProgress = 5))
        assertEquals(7.0, ListProgress.readThrough(savedChapter = 7.0, listProgress = 5))
    }

    @Test
    fun continueReadingMergesTheReadList() {
        val positions = listOf(pos(1, 3.0, completed = false, at = 1_000), pos(2, 4.0, completed = false, at = 3_000))
        val list = listOf(
            r(1, 2, 20, at = 500), // behind the saved chapter: resume the page
            r(2, 9, 20, at = 2_000), // ahead: chapter 10, details
            r(3, 4, null, at = 4_000), // list only
            r(4, 30, 30, at = 5_000), // finished: out
            r(5, 1, 10, at = 6_000, status = ListStatus.PLANNING), // not reading: out
        )
        val out = ListProgress.mergeContinueReading(positions, list, limit = 10)
        assertEquals(listOf("3", "2", "1"), out.map { it.mediaId })
        val (listOnly, ahead, saved) = out
        assertFalse(listOnly.resume)
        assertEquals(5.0, listOnly.chapter)
        assertEquals("c3", listOnly.poster)
        assertFalse(ahead.resume)
        assertEquals(10.0, ahead.chapter)
        assertEquals(3_000, ahead.updatedAtMs)
        assertTrue(saved.resume)
        assertEquals(3.0, saved.chapter)
        assertEquals(1, ListProgress.mergeContinueReading(positions, list, limit = 1).size)
    }
}
