package app.tatakai.next.feature.library

import app.cash.turbine.test
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.torrent.TorrentServiceStatus
import java.io.IOException
import java.time.Instant
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class LibraryViewModelTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private fun entry(id: Int, completed: Boolean, at: Long, ep: Int = 1) = HistoryEntry(
        animeId = "$id", title = "A$id", poster = null, episodeId = "$id?ep=$ep", episode = ep, progressSec = 10,
        durationSec = 100, completed = completed, watchedAt = Instant.ofEpochSecond(at), anilistId = id, malId = null,
    )

    private class FakeHistory(
        val continueSource: () -> Flow<List<HistoryEntry>>,
        val historySource: () -> Flow<List<HistoryEntry>> = continueSource,
        val sync: suspend () -> Result<Unit> = { Result.success(Unit) },
    ) : HistoryRepository {
        val synced = MutableStateFlow<Long?>(null)
        override fun continueWatching(limit: Int) = continueSource()
        override fun history(limit: Int) = historySource()
        override suspend fun record(entry: HistoryEntry) {}
        override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> = MutableStateFlow(emptyMap())
        override suspend fun syncNow(): Result<Unit> = sync().also { if (it.isSuccess) synced.value = 42L }
        override val lastSyncedAt: StateFlow<Long?> get() = synced
    }

    @Test
    fun `continue and history are sorted newest first`() = runTest {
        val repo = FakeHistory(
            continueSource = { MutableStateFlow(listOf(entry(1, true, 10), entry(2, false, 30))) },
            historySource = { MutableStateFlow(listOf(entry(3, false, 20, ep = 2), entry(3, true, 40, ep = 3), entry(1, true, 10))) },
        )
        val vm = LibraryViewModel(repo)

        val s = vm.state.first { it.continueWatching is LoadState.Ok && it.history is LoadState.Ok }

        assertEquals(listOf(2, 1), assertIs<LoadState.Ok<List<HistoryEntry>>>(s.continueWatching).value.map { it.anilistId })
        assertEquals(listOf(3, 2, 1), assertIs<LoadState.Ok<List<HistoryEntry>>>(s.history).value.map { it.episode })
    }

    @Test
    fun `empty history is reported as empty lists`() = runTest {
        val vm = LibraryViewModel(FakeHistory({ MutableStateFlow(emptyList()) }))
        val s = vm.state.first { it.continueWatching is LoadState.Ok && it.history is LoadState.Ok }
        assertTrue(assertIs<LoadState.Ok<List<HistoryEntry>>>(s.continueWatching).value.isEmpty())
        assertTrue(assertIs<LoadState.Ok<List<HistoryEntry>>>(s.history).value.isEmpty())
    }

    @Test
    fun `failure surfaces as error and retry recovers`() = runTest {
        var fail = true
        val repo = FakeHistory({
            flow {
                if (fail) throw IOException("db")
                emit(listOf(entry(1, false, 1)))
            }
        })
        val vm = LibraryViewModel(repo)

        val err = vm.state.first { it.continueWatching is LoadState.Err }
        assertEquals(AppError.Offline, assertIs<LoadState.Err>(err.continueWatching).cause)

        fail = false
        vm.retry()
        vm.state.first { it.continueWatching is LoadState.Ok && it.history is LoadState.Ok }
    }

    @Test
    fun `active downloads drive the torrents badge`() = runTest {
        val torrents = FakeTorrents()
        val vm = LibraryViewModel(FakeHistory({ MutableStateFlow(emptyList()) }), torrents)
        vm.state.test {
            assertTrue(awaitUntil { it.activeDownloads == 0 })
            torrents.status.value = TorrentServiceStatus(running = true, activeDownloads = 3)
            assertTrue(awaitUntil { it.activeDownloads == 3 })
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `sync now reports success, updates the last sync and ignores double taps`() = runTest {
        val gate = CompletableDeferred<Unit>()
        var calls = 0
        val repo = FakeHistory({ MutableStateFlow(emptyList()) }, sync = {
            calls++
            gate.await()
            Result.success(Unit)
        })
        val vm = LibraryViewModel(repo)
        vm.events.test {
            vm.state.test {
                vm.syncNow()
                vm.syncNow()
                assertTrue(awaitUntil { it.syncing })
                gate.complete(Unit)
                assertTrue(awaitUntil { !it.syncing && it.lastSyncedAt == 42L })
                cancelAndIgnoreRemainingEvents()
            }
            assertEquals(LibraryEvent.Synced, awaitItem())
        }
        assertEquals(1, calls)
    }

    @Test
    fun `sync failure is reported`() = runTest {
        val repo = FakeHistory({ MutableStateFlow(emptyList()) }, sync = { Result.failure(AppException(AppError.Offline)) })
        val vm = LibraryViewModel(repo)
        vm.events.test {
            vm.syncNow()
            assertEquals(LibraryEvent.SyncFailed(AppError.Offline), awaitItem())
        }
        assertEquals(false, vm.state.value.syncing)
    }

    private suspend fun app.cash.turbine.ReceiveTurbine<LibraryUiState>.awaitUntil(predicate: (LibraryUiState) -> Boolean): Boolean {
        while (true) {
            if (predicate(awaitItem())) return true
        }
    }
}
