package app.tatakai.next.feature.you

import app.cash.turbine.test
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.ConflictChoice
import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListConflict
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListSyncReport
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.RemoteSession
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import java.time.Instant
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

@OptIn(ExperimentalCoroutinesApi::class)
class YouViewModelTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    /** parity wave A (2026-11-09 amendment): identity = linked AniList / MAL accounts. Only `accounts` matters to the You screen. */
    private class FakeLists : ListsRepository {
        override val accounts = MutableStateFlow<List<LinkedAccount>>(emptyList())
        override val pendingChanges: StateFlow<Int> = MutableStateFlow(0)
        override val lastSyncedAt: StateFlow<Long?> = MutableStateFlow<Long?>(null)
        override fun entries(status: ListStatus?): Flow<List<ListEntry>> = flowOf(emptyList())
        override fun favorites(): Flow<List<ListEntry>> = flowOf(emptyList())
        override fun entry(ref: MediaRef): Flow<ListEntry?> = flowOf(null)
        override suspend fun setStatus(media: Media, status: ListStatus?): Result<Unit> = Result.success(Unit)
        override suspend fun setProgress(ref: MediaRef, episode: Int): Result<Unit> = Result.success(Unit)
        override suspend fun setScore(ref: MediaRef, score: Int?): Result<Unit> = Result.success(Unit)
        override suspend fun setFavorite(media: Media, favorite: Boolean): Result<Unit> = Result.success(Unit)
        override fun favoriteCharacters(): Flow<List<CharacterRef>> = flowOf(emptyList())
        override fun isCharacterFavorite(characterId: Int): Flow<Boolean> = flowOf(false)
        override suspend fun setCharacterFavorite(character: CharacterRef, favorite: Boolean): Result<Unit> = Result.success(Unit)
        override suspend fun beginLink(provider: ListProvider): Result<String> = Result.failure(UnsupportedOperationException())
        override suspend fun completeLink(redirectUri: String): Result<LinkedAccount> = Result.failure(UnsupportedOperationException())
        override suspend fun unlink(provider: ListProvider) {}
        override suspend fun syncNow(): Result<ListSyncReport> = Result.success(ListSyncReport(0, 0, 0, 0))
        override fun conflicts(): Flow<List<ListConflict>> = flowOf(emptyList())
        override suspend fun resolveConflict(id: String, choice: ConflictChoice) {}
        override suspend fun exportJson(): String = "[]"
        override suspend fun importJson(json: String): Result<Int> = Result.success(0)
    }

    private class FakeHistory(var failHistory: Boolean = false) : HistoryRepository {
        val rows = MutableStateFlow<List<HistoryEntry>>(emptyList())
        val synced = MutableStateFlow<Long?>(null)
        var syncResult: Result<Unit> = Result.success(Unit)
        override fun continueWatching(limit: Int): Flow<List<HistoryEntry>> = rows
        override suspend fun record(entry: HistoryEntry) {}
        override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> = emptyFlow()
        override fun history(limit: Int): Flow<List<HistoryEntry>> =
            if (failHistory) flow { throw AppException(AppError.Unknown("db")) } else rows
        override suspend fun syncNow(): Result<Unit> = syncResult.onSuccess { synced.value = 1_000L }
        override val lastSyncedAt: StateFlow<Long?> get() = synced
    }

    private class FakeExtensions : ExtensionRepository {
        override val installed = MutableStateFlow<List<InstalledExtension>>(emptyList())
        override suspend fun installFromUri(uri: String): Result<InstalledExtension> = Result.failure(UnsupportedOperationException())
        override suspend fun setEnabled(id: String, enabled: Boolean) {}
        override suspend fun remove(id: String) {}
        override suspend fun resolveStreams(media: Media, episode: Int): List<StreamSource> = emptyList()
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

    private fun entry(ep: Int) = HistoryEntry(
        animeId = "al:1", title = "T", poster = null, episodeId = "al:1?ep=$ep", episode = ep, progressSec = 10, durationSec = 100,
        completed = false, watchedAt = Instant.EPOCH, anilistId = 1, malId = null,
    )

    @Test
    fun `overview counts history, extensions, accounts and the linked pc`() = runTest {
        val lists = FakeLists()
        val history = FakeHistory()
        val extensions = FakeExtensions()
        val link = FakeLink()
        val vm = YouViewModel(lists, history, extensions, link)

        history.rows.value = listOf(entry(1), entry(2))
        extensions.installed.value = listOf(
            InstalledExtension("e1", "Toko", "1.0", "custom", emptyList(), ExtensionCompat.FULL, enabled = true),
        )
        lists.accounts.value = listOf(LinkedAccount(ListProvider.ANILIST, "1", "octavio", null, linkedAtMs = 0L, expiresAtMs = null))
        link.state.value = LinkState.Paired(PairedPc("d", "DESKTOP", "192.168.1.2", 47321), reachable = false)

        val ok = assertIs<LoadState.Ok<YouOverview>>(vm.state.first { it.overview is LoadState.Ok && (it.overview as LoadState.Ok).value.historyCount == 2 && (it.overview as LoadState.Ok).value.linked is LinkedSummary.Paired }.overview)
        assertEquals(1, ok.value.extensionsCount)
        assertEquals("octavio", ok.value.accounts.single().username)
        assertEquals(LinkedSummary.Paired("DESKTOP", reachable = false), ok.value.linked)
        assertEquals(0, ok.value.activeDownloads)
    }

    @Test
    fun `history failure shows the error and retry re-subscribes`() = runTest {
        val history = FakeHistory(failHistory = true)
        val vm = YouViewModel(FakeLists(), history, FakeExtensions(), FakeLink())

        assertIs<LoadState.Err>(vm.state.first { it.overview is LoadState.Err }.overview)

        history.failHistory = false
        vm.retry()

        assertIs<LoadState.Ok<YouOverview>>(vm.state.first { it.overview is LoadState.Ok }.overview)
    }

    @Test
    fun `sync now reports success and failure as events`() = runTest {
        val history = FakeHistory()
        val vm = YouViewModel(FakeLists(), history, FakeExtensions(), FakeLink())

        vm.events.test {
            vm.syncNow()
            assertEquals(YouEvent.Synced, awaitItem())

            history.syncResult = Result.failure(AppException(AppError.Offline))
            vm.syncNow()
            assertEquals(YouEvent.SyncFailed(AppError.Offline), awaitItem())
        }
        assertEquals(false, vm.state.value.syncing)
    }
}
