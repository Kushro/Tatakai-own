// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.HistoryDao
import app.tatakai.next.database.HistoryRow
import app.tatakai.next.link.LinkHistoryPageDto
import app.tatakai.next.link.LinkHistoryPushResultDto
import app.tatakai.next.link.LinkHistoryRemote
import app.tatakai.next.link.LinkHistoryRow
import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.RemoteSession
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

// F6 (parity wave A tests, 15 §5 q / history.link-sync.client): when the Link history sync is requested (app open, new pairing) and the
// stamp + cursor reset on unpair; in-memory HistorySyncStamp / HistoryCursorStore (store = null).
@OptIn(ExperimentalCoroutinesApi::class)
class HistorySyncTriggersTest {
    private class RecordingScheduler : HistorySyncScheduler {
        val requests = mutableListOf<Boolean>()
        override fun requestSync(immediate: Boolean) {
            requests += immediate
        }
    }

    private class FakeLink : LinkRepository {
        override val state = MutableStateFlow<LinkState>(LinkState.Unpaired)
        override fun discover(): Flow<List<DiscoveredPc>> = emptyFlow()
        override suspend fun pair(payload: String, deviceName: String): Result<PairedPc> = Result.failure(UnsupportedOperationException())
        override suspend fun capabilities(): Result<LinkCapabilities> = Result.failure(UnsupportedOperationException())
        override suspend fun startSession(source: LinkSource, profileId: String?): Result<RemoteSession> =
            Result.failure(UnsupportedOperationException())
        override suspend fun stop(sessionId: String) {}
        override suspend fun forget() {}
    }

    /** resetStamp never touches the dao or the remote; these only satisfy the constructor. */
    private class NoDao : HistoryDao() {
        override fun observeRecent(limit: Int): Flow<List<HistoryRow>> = flowOf(emptyList())
        override fun observeForAnime(anilistId: Int, animeId: String): Flow<List<HistoryRow>> = flowOf(emptyList())
        override suspend fun get(episodeId: String): HistoryRow? = null
        override suspend fun dirty(limit: Int): List<HistoryRow> = emptyList()
        override suspend fun dirtyCount(): Int = 0
        override suspend fun upsert(row: HistoryRow) {}
        override suspend fun markSynced(episodeId: String, watchedAtMs: Long) {}
        override suspend fun clear() {}
        override suspend fun all(): List<HistoryRow> = emptyList()
    }

    private class NoRemote : LinkHistoryRemote {
        override var available: Boolean = true
        override suspend fun push(rows: List<LinkHistoryRow>): LinkHistoryPushResultDto = LinkHistoryPushResultDto(rows.size)
        override suspend fun since(sinceUnix: Long, limit: Int): LinkHistoryPageDto = LinkHistoryPageDto(emptyList(), sinceUnix)
        override suspend fun clear() {}
    }

    private val pc = PairedPc("d1", "PC", "192.168.1.5", 47321)

    @Test
    fun syncsAtStartAndWhenAPcGetsPairedAndResetsOnUnpair() = runTest {
        val scheduler = RecordingScheduler()
        val link = FakeLink()
        val stamp = HistorySyncStamp()
        val savedCursors = mutableListOf<Long>()
        val syncer = HistorySyncer(NoDao(), NoRemote(), stamp = stamp, saveCursor = { savedCursors += it })
        stamp.set(1234L)
        val scope = CoroutineScope(backgroundScope.coroutineContext + UnconfinedTestDispatcher(testScheduler))

        HistorySyncTriggers(scheduler, link, syncer, scope).start()
        assertEquals(listOf(true), scheduler.requests, "app open = one immediate sync")

        link.state.value = LinkState.Pairing
        assertEquals(1, scheduler.requests.size, "pairing in progress does not sync")

        link.state.value = LinkState.Paired(pc, reachable = true)
        assertEquals(listOf(true, true), scheduler.requests, "a new pairing pulls the PC history at once")

        link.state.value = LinkState.Paired(pc, reachable = false)
        assertEquals(2, scheduler.requests.size, "reachability changes of the same pairing are not a new pairing")
        assertEquals(1234L, stamp.value.value)

        link.state.value = LinkState.Unpaired
        assertNull(stamp.value.value, "forgetting the PC clears the stamp")
        assertEquals(listOf(0L), savedCursors, "and rewinds the cursor")

        link.state.value = LinkState.Paired(pc, reachable = true)
        assertEquals(3, scheduler.requests.size)
        assertTrue(scheduler.requests.all { it })
    }

    @Test
    fun startingWhilePairedDoesNotDoubleSync() = runTest {
        val scheduler = RecordingScheduler()
        val link = FakeLink().apply { state.value = LinkState.Paired(pc, reachable = true) }
        val scope = CoroutineScope(backgroundScope.coroutineContext + UnconfinedTestDispatcher(testScheduler))
        HistorySyncTriggers(scheduler, link, syncer = null, scope = scope).start()
        assertEquals(listOf(true), scheduler.requests)
        link.state.value = LinkState.Unpaired
        link.state.value = LinkState.Paired(pc, reachable = true)
        assertEquals(listOf(true, true), scheduler.requests)
    }

    @Test
    fun withoutLinkOnlyTheStartupSyncIsRequested() = runTest {
        val scheduler = RecordingScheduler()
        HistorySyncTriggers(scheduler, link = null, syncer = null, scope = backgroundScope).start()
        assertEquals(listOf(true), scheduler.requests)
    }

    @Test
    fun inMemoryStampAndCursor() = runTest {
        val stamp = HistorySyncStamp()
        assertNull(stamp.value.value)
        stamp.set(10L)
        assertEquals(10L, stamp.value.value)
        stamp.set(null)
        assertNull(stamp.value.value)

        val cursor = HistoryCursorStore()
        assertEquals(0L, cursor.value())
        cursor.set(42L)
        assertEquals(42L, cursor.value())
        cursor.set(-5L)
        assertEquals(0L, cursor.value(), "negative cursors are clamped")
    }
}
