// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.Episode
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.MediaRef
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 0.4.1: what the end-of-episode screen shows for the engine / view model state. */
class EndScreenStateTest {
    private fun state(
        ended: Boolean = true,
        loop: Boolean = false,
        started: Boolean = true,
        episode: Int = 5,
        total: Int? = 12,
        hasNext: Boolean = true,
        canNavigate: Boolean = true,
        next: NextEpisodeUi? = null,
        episodes: List<Episode> = emptyList(),
    ) = endScreenState(ended, loop, started, episode, total, hasNext, canNavigate, next, episodes)

    @Test
    fun onlyAtTheRealEnd() {
        assertNull(state(ended = false), "still playing")
        assertNull(state(loop = true), "looping never ends")
        assertNull(state(started = false), "a load that never showed a frame is not an end")
        assertNotNull(state())
    }

    @Test
    fun nextEpisodeWithTheAutoNextCountdown() {
        val ui = state(next = NextEpisodeUi(episode = 6, secondsLeft = 7, sameTorrent = true, totalSecs = 10))!!
        val next = ui.next!!
        assertEquals(6, next.episode)
        assertEquals(7, next.secondsLeft)
        assertEquals(10, next.totalSecs)
        assertTrue(next.sameTorrent)
        assertEquals(5, ui.episode)
        assertFalse(ui.seriesFinished)
        assertTrue(ui.canBrowseEpisodes)
    }

    @Test
    fun autoNextOffOrCancelledStillOffersTheNextEpisode() {
        // auto-next off: the view model's card without seconds
        val off = state(next = NextEpisodeUi(episode = 6, secondsLeft = null, sameTorrent = false, totalSecs = 15))!!.next!!
        assertNull(off.secondsLeft)
        assertEquals(15, off.totalSecs)
        // Cancel cleared the card: the button stays, nothing counts
        val cancelled = state(next = null)!!.next!!
        assertEquals(6, cancelled.episode)
        assertNull(cancelled.secondsLeft)
    }

    @Test
    fun nextEpisodeTitleAndThumbnailFromTheEpisodeList() {
        val eps = listOf(Episode(5, "Five", "img5", null), Episode(6, "The sixth", "img6", null))
        val next = state(episodes = eps)!!.next!!
        assertEquals("The sixth", next.title)
        assertEquals("img6", next.image)
        // blank values are "unknown"
        assertNull(state(episodes = listOf(Episode(6, " ", "", null)))!!.next!!.title)
    }

    @Test
    fun lastEpisodeOfTheSeries() {
        val ui = state(episode = 12, total = 12, hasNext = false)!!
        assertNull(ui.next)
        assertTrue(ui.seriesFinished)
    }

    @Test
    fun noNextWithAnUnknownTotalIsNotTheEndOfTheSeries() {
        val ui = state(episode = 3, total = null, hasNext = false)!!
        assertNull(ui.next)
        assertFalse(ui.seriesFinished)
    }

    @Test
    fun aPcLibraryFileIsJustAVideo() {
        // Link: a file of the PC's library has no title the phone can look up (no next, no list, no "Episodio N")
        val ui = state(canNavigate = false, hasNext = true)!!
        assertNull(ui.episode)
        assertNull(ui.next)
        assertFalse(ui.seriesFinished)
        assertFalse(ui.canBrowseEpisodes)
    }

    private fun entry(progress: Int, status: ListStatus = ListStatus.WATCHING, total: Int? = 12) =
        ListEntry(MediaRef(anilistId = 1), "T", null, status, progress = progress, totalEpisodes = total, updatedAtMs = 0)

    private val anilist = LinkedAccount(ListProvider.ANILIST, "1", "me", null, 0, null)

    @Test
    fun progressLine() {
        assertNull(endProgress(trackable = false, episode = 5, entry = null, autoListProgress = true, accounts = emptyList()))
        assertEquals(EndProgressKind.WATCHED, endProgress(true, 5, null, true, emptyList())!!.kind, "not on the list: the history only")
        assertEquals(EndProgressKind.WATCHED, endProgress(true, 5, entry(4), true, emptyList())!!.kind, "the list was not advanced (yet)")
        assertEquals(EndProgressKind.WATCHED, endProgress(true, 5, entry(5), false, emptyList())!!.kind, "automatic progress off")
        assertEquals(EndProgressKind.WATCHED, endProgress(true, 3, entry(8), true, emptyList())!!.kind, "a rewatch leaves the list alone")

        val updated = endProgress(true, 5, entry(5), true, listOf(anilist))!!
        assertEquals(EndProgressKind.LIST_UPDATED, updated.kind)
        assertEquals(5, updated.progress)
        assertEquals(12, updated.total)
        assertEquals(listOf(ListProvider.ANILIST), updated.syncsWith)

        val done = endProgress(true, 12, entry(12, ListStatus.COMPLETED), true, emptyList())!!
        assertEquals(EndProgressKind.LIST_COMPLETED, done.kind)
        assertTrue(done.syncsWith.isEmpty(), "no linked account: local list only")
    }
}
