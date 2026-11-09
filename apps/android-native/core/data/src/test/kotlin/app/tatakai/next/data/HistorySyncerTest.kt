// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.HistoryDao
import app.tatakai.next.database.HistoryRow
import app.tatakai.next.link.LinkError
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
            return LinkHistoryPushResultDto(rows.size)
        }
        override suspend fun since(sinceUnix: Long, limit: Int): LinkHistoryPageDto {
            sinceAsked += sinceUnix
            return if (pages.isEmpty()) LinkHistoryPageDto(emptyList(), sinceUnix) else pages.removeAt(0)
        }
        override suspend fun clear() {}
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
}
