// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.ProviderEntry
import app.tatakai.next.model.ProviderItem
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave B (16 §3.4, worker t): pure helpers of the provider pages.
class ProviderHelpersTest {
    private val entries = listOf(
        ProviderEntry("e3", "watch", number = 3.0, title = "The third"),
        ProviderEntry("e1", "watch", number = 1.0, title = "Pilot"),
        ProviderEntry("e2", "watch", number = 2.0),
        ProviderEntry("sp", "watch", number = null, title = "Special"),
    )

    private fun row(animeId: String, episode: Int, completed: Boolean, at: Long, anilistId: Int? = null) = HistoryEntry(
        animeId = animeId, title = "T", poster = null, episodeId = "$animeId?ep=$episode", episode = episode, progressSec = 300,
        durationSec = 1400, completed = completed, watchedAt = Instant.ofEpochSecond(at), anilistId = anilistId, malId = null,
    )

    @Test
    fun sortsByNumberAndKeepsUnnumberedLast() {
        assertEquals(listOf("e1", "e2", "e3", "sp"), filterAndSort(entries, "", EntrySort.ASC).map { it.id })
        assertEquals(listOf("sp", "e3", "e2", "e1"), filterAndSort(entries, "", EntrySort.DESC).map { it.id })
    }

    @Test
    fun filtersByTitleOrNumber() {
        assertEquals(listOf("e1"), filterAndSort(entries, "pilot", EntrySort.ASC).map { it.id })
        assertEquals(listOf("e2"), filterAndSort(entries, "2", EntrySort.ASC).map { it.id })
        assertTrue(filterAndSort(entries, "zzz", EntrySort.ASC).isEmpty())
    }

    @Test
    fun episodeNumberFallsBackToPosition() {
        assertEquals(3, entryEpisode(entries, entries[0]))
        assertEquals(4, entryEpisode(entries, entries[3]))
        assertEquals("12", formatNumber(12.0))
        assertEquals("12.5", formatNumber(12.5))
    }

    @Test
    fun playerArgsCarryTheProviderRefAndResume() {
        val item = ProviderItem("item-1", "Show", entries = entries)
        val progress = mapOf(2 to row("ext:x:s:item-1", 2, completed = false, at = 10))
        val args = providerPlayerArgs("x", "s", item, entries[2], progress)
        assertEquals(2, args.episode)
        assertEquals(300_000L, args.startMs)
        assertEquals("ext:x:s:item-1", args.provider?.historyId)
        assertEquals("e2", args.provider?.entryId)
        assertEquals(MediaRef(), args.media)
        val matched = providerPlayerArgs("x", "s", item.copy(match = MediaRef(anilistId = 9)), entries[1])
        assertEquals(9, matched.media.anilistId)
        assertEquals(0L, matched.startMs)
    }

    @Test
    fun continueTargetIsTheNextAfterACompletedEpisode() {
        val done = mapOf(1 to row("h", 1, completed = true, at = 1), 2 to row("h", 2, completed = true, at = 2))
        assertEquals("e3", continueTarget(entries, done)?.id)
        val half = mapOf(1 to row("h", 1, completed = true, at = 1), 2 to row("h", 2, completed = false, at = 5))
        assertEquals("e2", continueTarget(entries, half)?.id)
        assertNull(continueTarget(entries, emptyMap()))
    }

    @Test
    fun progressKeepsOnlyThisTitleLatestRowPerEpisode() {
        val rows = listOf(
            row("ext:x:s:i", 1, completed = false, at = 1),
            row("ext:x:s:i", 1, completed = true, at = 9),
            row("ext:x:s:other", 2, completed = true, at = 9),
            row("77", 3, completed = false, at = 3, anilistId = 77),
        )
        val p = progressOf(rows, "ext:x:s:i", anilistId = null)
        assertEquals(setOf(1), p.keys)
        assertTrue(p.getValue(1).completed)
        assertEquals(setOf(1, 3), progressOf(rows, "ext:x:s:i", anilistId = 77).keys)
    }

    @Test
    fun urlCheck() {
        assertTrue(isHttpUrl("https://example.com/index.json"))
        assertTrue(isHttpUrl("  http://192.168.1.2:8080/x.kai "))
        assertFalse(isHttpUrl("ftp://example.com"))
        assertFalse(isHttpUrl("https://"))
        assertFalse(isHttpUrl("https://exa mple.com"))
    }
}
