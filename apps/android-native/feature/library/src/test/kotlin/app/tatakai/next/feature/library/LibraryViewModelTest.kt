package app.tatakai.next.feature.library

import app.tatakai.next.model.AppError
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import java.io.IOException
import java.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
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

    private fun entry(id: Int, completed: Boolean, at: Long) = HistoryEntry(
        animeId = "$id", title = "A$id", poster = null, episodeId = "$id?ep=1", episode = 1, progressSec = 10,
        durationSec = 100, completed = completed, watchedAt = Instant.ofEpochSecond(at), anilistId = id, malId = null,
    )

    private class FakeHistory(val source: () -> Flow<List<HistoryEntry>>) : HistoryRepository {
        override fun continueWatching(limit: Int) = source()
        override suspend fun record(entry: HistoryEntry) {}
        override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> = MutableStateFlow(emptyMap())
    }

    @Test
    fun `groups unfinished entries and sorts history by recency`() = runTest {
        val repo = FakeHistory { MutableStateFlow(listOf(entry(1, true, 10), entry(2, false, 30), entry(3, false, 20))) }
        val vm = LibraryViewModel(repo)

        val data = assertIs<LoadState.Ok<LibraryData>>(vm.state.first { it !is LoadState.Loading }).value

        assertEquals(listOf(2, 3), data.inProgress.map { it.anilistId })
        assertEquals(listOf(2, 3, 1), data.history.map { it.anilistId })
    }

    @Test
    fun `empty history is reported as empty`() = runTest {
        val vm = LibraryViewModel(FakeHistory { MutableStateFlow(emptyList()) })
        val data = assertIs<LoadState.Ok<LibraryData>>(vm.state.first { it !is LoadState.Loading }).value
        assertTrue(data.isEmpty)
    }

    @Test
    fun `failure surfaces as error and retry recovers`() = runTest {
        var fail = true
        val repo = FakeHistory {
            flow {
                if (fail) throw IOException("db")
                emit(listOf(entry(1, false, 1)))
            }
        }
        val vm = LibraryViewModel(repo)

        val err = assertIs<LoadState.Err>(vm.state.first { it !is LoadState.Loading })
        assertEquals(AppError.Offline, err.cause)

        fail = false
        vm.retry()
        assertIs<LoadState.Ok<LibraryData>>(vm.state.first { it is LoadState.Ok })
    }
}
