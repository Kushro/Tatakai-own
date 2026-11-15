// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.HistoryDao
import app.tatakai.next.database.HistoryRow
import app.tatakai.next.link.HistoryResyncMode
import app.tatakai.next.link.LinkError
import app.tatakai.next.link.LinkHistoryMode
import app.tatakai.next.link.LinkHistoryPageDto
import app.tatakai.next.link.LinkHistoryPushResultDto
import app.tatakai.next.link.LinkHistoryRemote
import app.tatakai.next.link.LinkHistoryRow
import app.tatakai.next.link.LinkHistoryUnavailableException
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.runBlocking
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

/** parity wave A (worker q, history.link-sync.client / merge): push in batches, pull with `truncated`, cursor, unpaired = no-op. */
class HistorySyncerTest {

    private class FakeDao : HistoryDao() {
        val rows = MutableStateFlow<Map<String, HistoryRow>>(emptyMap())
        override fun observeRecent(limit: Int): Flow<List<HistoryRow>> = rows.map { it.values.sortedByDescending { r -> r.watchedAtMs }.take(limit) }
        override fun observeForAnime(anilistId: Int, animeId: String): Flow<List<HistoryRow>> =
            rows.map { m -> m.values.filter { it.anilistId == anilistId || it.animeId == animeId } }
        override suspend fun get(episodeId: String): HistoryRow? = rows.value[episodeId]
        override suspend fun dirty(limit: Int): List<HistoryRow> = rows.value.values.filter { it.isDirty() }.sortedBy { it.watchedAtMs }.take(limit)
        override suspend fun dirtyCount(): Int = rows.value.values.count { it.isDirty() }
        override suspend fun upsert(row: HistoryRow) {
            rows.value = rows.value + (row.episodeId to row)
        }
        override suspend fun markSynced(episodeId: String, watchedAtMs: Long) {
            val r = rows.value[episodeId] ?: return
            if (r.watchedAtMs == watchedAtMs) upsert(r.copy(syncedAtMs = watchedAtMs))
        }
        override suspend fun clear() {
            rows.value = emptyMap()
        }
        override suspend fun all(): List<HistoryRow> = rows.value.values.toList()
    }

    private class FakeRemote(var pages: MutableList<LinkHistoryPageDto> = mutableListOf()) : LinkHistoryRemote {
        override var available: Boolean = true
        val pushed = mutableListOf<List<LinkHistoryRow>>()
        val sinceAsked = mutableListOf<Long>()
        var unavailable = false
        override suspend fun push(rows: List<LinkHistoryRow>): LinkHistoryPushResultDto {
            if (unavailable) throw LinkHistoryUnavailableException(LinkError.Transport.Tls.Revoked)
            pushed += rows
            calls += "push"
            return LinkHistoryPushResultDto(rows.size)
        }
        override suspend fun since(sinceUnix: Long, limit: Int): LinkHistoryPageDto {
            sinceAsked += sinceUnix
            calls += "since"
            return if (pages.isEmpty()) LinkHistoryPageDto(emptyList(), sinceUnix) else pages.removeAt(0)
        }
        override suspend fun clear() {}

        // 2026-11 history per device
        var pcMode: LinkHistoryMode? = LinkHistoryMode.FULL
        var resyncDoneCalls = 0
        val resyncNotes = mutableListOf<Boolean>()
        val calls = mutableListOf<String>()
        override suspend fun mode(): LinkHistoryMode? = pcMode.also { calls += "mode" }
        override suspend fun resyncDone() {
            resyncDoneCalls++
            calls += "done"
        }
        override fun noteResync(running: Boolean) {
            resyncNotes += running
        }
    }

    private fun row(i: Int, watched: Long, synced: Long? = null, progress: Int = 60) = HistoryRow(
        episodeId = "1:$i",
        animeId = "1",
        title = "Frieren",
        poster = null,
        episode = i,
        progressSec = progress,
        durationSec = 1440,
        completed = false,
        watchedAtMs = watched,
        anilistId = 1,
        malId = null,
        syncedAtMs = synced,
    )

    private fun remoteRow(i: Int, watchedIso: String, progress: Int) = LinkHistoryRow(
        animeId = "1", title = "Frieren", episodeId = "1:$i", episode = i, progressSec = progress, durationSec = 1440, watchedAt = watchedIso, anilistId = 1,
    )

    @Test
    fun pushesDirtyRowsInBatchesOf100AndMarksThemClean() = runBlocking {
        val dao = FakeDao()
        repeat(250) { dao.upsert(row(it, watched = 1_000L + it)) }
        dao.upsert(row(999, watched = 5_000, synced = 5_000))
        val remote = FakeRemote()
        val syncer = HistorySyncer(dao, remote)
        assertEquals(HistorySyncer.Outcome.SYNCED, syncer.syncOnce())
        assertEquals(listOf(100, 100, 50), remote.pushed.map { it.size })
        assertEquals(0, dao.dirtyCount())
        assertTrue(remote.pushed.flatten().none { it.episodeId == "1:999" }, "clean rows are not sent")
    }

    @Test
    fun pullsUntilNotTruncatedMergesAndSavesTheCursor() = runBlocking {
        val dao = FakeDao()
        // local row older than the remote one: the PC's progress wins
        dao.upsert(row(1, watched = 1_700_000_000_000L, synced = 1_700_000_000_000L, progress = 10))
        val remote = FakeRemote(
            mutableListOf(
                LinkHistoryPageDto(listOf(remoteRow(1, "2026-11-01T10:00:00.000Z", 180)), nextSinceUnix = 100, truncated = true),
                LinkHistoryPageDto(listOf(remoteRow(2, "2026-11-01T11:00:00.000Z", 30)), nextSinceUnix = 200, truncated = false),
            ),
        )
        var saved = -1L
        val syncer = HistorySyncer(dao, remote, cursor = { 42L }, saveCursor = { saved = it }, clock = { 7L })
        syncer.syncOnce()
        assertEquals(listOf(42L, 100L), remote.sinceAsked)
        assertEquals(200L, saved)
        assertEquals(180, dao.get("1:1")!!.progressSec)
        assertEquals(30, dao.get("1:2")!!.progressSec)
        assertEquals(0, dao.dirtyCount(), "merged rows are clean")
        assertEquals(7L, syncer.lastSyncedAt.value)
    }

    @Test
    fun newerLocalRowSurvivesTheMergeAndIsPushed() = runBlocking {
        val dao = FakeDao()
        val localWatched = java.time.Instant.parse("2026-11-02T00:00:00Z").toEpochMilli()
        dao.upsert(row(1, watched = localWatched, progress = 900))
        val remote = FakeRemote(mutableListOf(LinkHistoryPageDto(listOf(remoteRow(1, "2026-11-01T00:00:00.000Z", 5)), 10)))
        HistorySyncer(dao, remote).syncOnce()
        assertEquals(900, dao.get("1:1")!!.progressSec)
        assertEquals(1, remote.pushed.flatten().size)
    }

    @Test
    fun aNewerRowWithoutAPosterKeepsTheLocalCover() = runBlocking {
        // BUG-096: the PC's newer row (played in a Link session) came without a poster and wiped the cover
        val dao = FakeDao()
        dao.upsert(row(1, watched = 1_700_000_000_000L, synced = 1_700_000_000_000L).copy(poster = "https://img/frieren.jpg"))
        HistorySyncer(dao, FakeRemote(mutableListOf(LinkHistoryPageDto(listOf(remoteRow(1, "2026-11-01T10:00:00.000Z", 180)), 10)))).syncOnce()
        assertEquals(180, dao.get("1:1")!!.progressSec)
        assertEquals("https://img/frieren.jpg", dao.get("1:1")!!.poster)
    }

    @Test
    fun recordingWithoutAPosterKeepsTheEpisodeOrSeriesCover() = runBlocking {
        // BUG-096: the player of a Link session records progress before (or without) the metadata
        val dao = FakeDao()
        val repo = HistoryRepositoryImpl(dao, object : HistorySyncScheduler {
            override fun requestSync(immediate: Boolean) {}
        })
        dao.upsert(row(1, watched = 1_000).copy(poster = "https://img/ep1.jpg"))
        fun entry(ep: Int, poster: String?) = app.tatakai.next.model.HistoryEntry(
            animeId = "1", title = "Frieren", poster = poster, episodeId = "1:$ep", episode = ep, progressSec = 300, durationSec = 1440,
            completed = false, watchedAt = java.time.Instant.ofEpochMilli(2_000L + ep), anilistId = 1, malId = null,
        )
        repo.record(entry(1, null))
        assertEquals("https://img/ep1.jpg", dao.get("1:1")!!.poster, "the episode keeps its cover")
        repo.record(entry(2, null))
        assertEquals("https://img/ep1.jpg", dao.get("1:2")!!.poster, "a new episode takes the series' cover")
        repo.record(entry(2, "https://img/new.jpg"))
        assertEquals("https://img/new.jpg", dao.get("1:2")!!.poster, "a poster that comes with the row wins")
    }

    @Test
    fun unpairedOrDisabledDoesNothing() = runBlocking {
        val dao = FakeDao()
        dao.upsert(row(1, watched = 1))
        val remote = FakeRemote().apply { available = false }
        assertEquals(HistorySyncer.Outcome.SKIPPED_NOT_PAIRED, HistorySyncer(dao, remote).syncOnce())
        assertEquals(HistorySyncer.Outcome.SKIPPED_DISABLED, HistorySyncer(dao, FakeRemote(), enabled = { false }).syncOnce())
        assertEquals(1, dao.dirtyCount())
    }

    @Test
    fun revokedPairingSurfacesAsUnavailable() {
        val dao = FakeDao()
        runBlocking { dao.upsert(row(1, watched = 1)) }
        val remote = FakeRemote().apply { unavailable = true }
        assertFailsWith<LinkHistoryUnavailableException> { runBlocking { HistorySyncer(dao, remote).syncOnce() } }
    }

    // ---- 2026-11 history per device ----

    @Test
    fun pushAllSendsEveryRowThenSaysDone() = runBlocking {
        val dao = FakeDao()
        repeat(120) { dao.upsert(row(it, watched = 1_000L + it, synced = 1_000L + it)) }
        dao.upsert(row(500, watched = 9_000)) // one dirty row
        val remote = FakeRemote()
        var saved = -1L
        val syncer = HistorySyncer(dao, remote, cursor = { 77L }, saveCursor = { saved = it })
        assertEquals(HistorySyncer.Outcome.SYNCED, syncer.resync(HistoryResyncMode.PUSH_ALL))
        assertEquals(121, remote.pushed.flatten().size, "clean rows go too")
        assertEquals(1, remote.resyncDoneCalls)
        assertEquals(listOf("push", "push", "done", "since"), remote.calls, "done only after every push")
        assertEquals(listOf(77L), remote.sinceAsked, "then a normal pull")
        assertEquals(77L, saved)
        assertEquals(0, dao.dirtyCount())
        assertEquals(listOf(true, false), remote.resyncNotes)
    }

    @Test
    fun pullReplacePushesDirtyRowsFirstThenReplacesEverything() = runBlocking {
        val dao = FakeDao()
        dao.upsert(row(1, watched = 1_000)) // dirty: the PC should get it before the drop
        dao.upsert(row(2, watched = 2_000, synced = 2_000)) // clean, not on the PC page: gone afterwards
        val remote = FakeRemote(mutableListOf(LinkHistoryPageDto(listOf(remoteRow(1, "2026-11-01T10:00:00.000Z", 180)), nextSinceUnix = 300)))
        var saved = mutableListOf<Long>()
        val syncer = HistorySyncer(dao, remote, cursor = { 999L }, saveCursor = { saved += it })
        syncer.resync(HistoryResyncMode.PULL_REPLACE)
        assertEquals(listOf("mode", "push", "since"), remote.calls)
        assertEquals(listOf("1:1"), remote.pushed.flatten().map { it.episodeId })
        assertEquals(listOf(0L), remote.sinceAsked, "everything is pulled again")
        assertEquals(listOf(0L, 300L), saved)
        assertEquals(setOf("1:1"), dao.rows.value.keys, "the local history is the PC's now")
        assertEquals(180, dao.get("1:1")!!.progressSec)
        assertEquals(0, remote.resyncDoneCalls)
    }

    @Test
    fun pullReplaceOfAReceiveOnlyPhoneDoesNotPushFirst() = runBlocking {
        val dao = FakeDao()
        dao.upsert(row(1, watched = 1_000))
        val remote = FakeRemote().apply { pcMode = LinkHistoryMode.RECEIVE_ONLY }
        HistorySyncer(dao, remote).resync(HistoryResyncMode.PULL_REPLACE)
        assertTrue(remote.pushed.isEmpty())
        assertTrue(dao.rows.value.isEmpty())
        assertEquals(listOf(true, false), remote.resyncNotes)
    }

    @Test
    fun aFailedResyncStillEndsTheNotice() {
        val dao = FakeDao()
        runBlocking { dao.upsert(row(1, watched = 1)) }
        val remote = FakeRemote().apply { unavailable = true }
        assertFailsWith<LinkHistoryUnavailableException> { runBlocking { HistorySyncer(dao, remote).resync(HistoryResyncMode.PUSH_ALL) } }
        assertEquals(listOf(true, false), remote.resyncNotes)
        assertEquals(0, remote.resyncDoneCalls, "no done without the pushes")
        assertEquals(HistorySyncer.Outcome.SKIPPED_DISABLED, runBlocking { HistorySyncer(dao, FakeRemote(), enabled = { false }).resync(HistoryResyncMode.PULL_REPLACE) })
    }
}
