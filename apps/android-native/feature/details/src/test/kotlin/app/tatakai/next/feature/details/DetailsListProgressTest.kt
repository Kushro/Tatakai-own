package app.tatakai.next.feature.details

import app.cash.turbine.test
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.ConflictChoice
import app.tatakai.next.model.Episode
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
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
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.RemoteSession
import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.StreamSource
import java.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Details with the list progress of the local AniList / MAL mirror: resume episode, watched marks, manual toggle. */
@OptIn(ExperimentalCoroutinesApi::class)
class DetailsListProgressTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private val ref = MediaRef(anilistId = 7)
    private val anime = Media(
        ref = ref, kind = MediaKind.ANIME, title = "Seven", titles = emptyMap(), cover = null, banner = null, color = null,
        synopsis = null, genres = emptyList(), episodes = 12, status = null, season = null, year = null, score = null, sourceProvider = "fake",
    )

    private fun h(ep: Int, sec: Int, completed: Boolean, at: Long) = HistoryEntry(
        animeId = "7", title = "Seven", poster = null, episodeId = "7?ep=$ep", episode = ep, progressSec = sec,
        durationSec = 1400, completed = completed, watchedAt = Instant.ofEpochSecond(at), anilistId = 7, malId = null,
    )

    private class Metadata(val media: Media) : MetadataRepository {
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int) = FeedPage(emptyList(), page, false)
        override suspend fun search(kind: MediaKind, query: String, page: Int) = FeedPage(emptyList(), page, false)
        override suspend fun media(kind: MediaKind, ref: MediaRef) = media
        override suspend fun episodes(ref: MediaRef) = (1..12).map { Episode(it, "Ep $it", null, null) }
    }

    private class History(progress: Map<Int, HistoryEntry>) : HistoryRepository {
        val progress = MutableStateFlow(progress)
        val cleared = mutableListOf<Pair<MediaRef, Int>>()
        override fun continueWatching(limit: Int): Flow<List<HistoryEntry>> = emptyFlow()
        override suspend fun record(entry: HistoryEntry) {}
        override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> = progress
        override suspend fun clearCompleted(ref: MediaRef, fromEpisode: Int) {
            cleared += ref to fromEpisode
        }
    }

    private class Extensions : ExtensionRepository {
        override val installed: StateFlow<List<InstalledExtension>> = MutableStateFlow(
            listOf(InstalledExtension("ext", "Ext", "1.0", "anime", emptyList(), ExtensionCompat.FULL, true)),
        )
        override suspend fun installFromUri(uri: String) = Result.failure<InstalledExtension>(UnsupportedOperationException())
        override suspend fun setEnabled(id: String, enabled: Boolean) {}
        override suspend fun remove(id: String) {}
        override suspend fun resolveStreams(media: Media, episode: Int) = emptyList<StreamSource>()
        override suspend fun resolveCandidates(media: Media, episode: Int) = emptyList<RankedSource>()
    }

    private class Link : LinkRepository {
        override val state = MutableStateFlow<LinkState>(LinkState.Unpaired)
        override fun discover(): Flow<List<DiscoveredPc>> = emptyFlow()
        override suspend fun pair(payload: String, deviceName: String) = Result.failure<PairedPc>(UnsupportedOperationException())
        override suspend fun capabilities() = Result.failure<LinkCapabilities>(UnsupportedOperationException())
        override suspend fun startSession(source: LinkSource, profileId: String?) = Result.failure<RemoteSession>(UnsupportedOperationException())
        override suspend fun stop(sessionId: String) {}
        override suspend fun forget() {}
    }

    /** The local list mirror: setStatus / setProgress apply the same progress rule as ListsRepositoryImpl (enough for the VM). */
    private class Lists(initial: ListEntry?) : ListsRepository {
        val row = MutableStateFlow(initial)
        val calls = mutableListOf<String>()
        override val accounts: StateFlow<List<LinkedAccount>> = MutableStateFlow(emptyList())
        override val pendingChanges: StateFlow<Int> = MutableStateFlow(0)
        override val lastSyncedAt: StateFlow<Long?> = MutableStateFlow(null)
        override fun entries(status: ListStatus?): Flow<List<ListEntry>> = row.map { listOfNotNull(it) }
        override fun favorites(): Flow<List<ListEntry>> = flowOf(emptyList())
        override fun entry(ref: MediaRef): Flow<ListEntry?> = row
        override suspend fun setStatus(media: Media, status: ListStatus?): Result<Unit> {
            calls += "status:${status?.id}"
            row.value = status?.let { row.value?.copy(status = it) ?: ListEntry(media.ref, media.title, null, it, 0, media.episodes, updatedAtMs = 1) }
            return Result.success(Unit)
        }
        override suspend fun setProgress(ref: MediaRef, episode: Int): Result<Unit> {
            calls += "progress:$episode"
            val prev = row.value ?: return Result.failure(IllegalStateException("not on list"))
            row.value = prev.copy(progress = episode, status = if (prev.totalEpisodes == episode) ListStatus.COMPLETED else prev.status)
            return Result.success(Unit)
        }
        override suspend fun setScore(ref: MediaRef, score: Int?) = Result.success(Unit)
        override suspend fun setFavorite(media: Media, favorite: Boolean) = Result.success(Unit)
        override fun favoriteCharacters(): Flow<List<CharacterRef>> = flowOf(emptyList())
        override fun isCharacterFavorite(characterId: Int): Flow<Boolean> = flowOf(false)
        override suspend fun setCharacterFavorite(character: CharacterRef, favorite: Boolean) = Result.success(Unit)
        override suspend fun beginLink(provider: ListProvider) = Result.failure<String>(UnsupportedOperationException())
        override suspend fun completeLink(redirectUri: String) = Result.failure<LinkedAccount>(UnsupportedOperationException())
        override suspend fun unlink(provider: ListProvider) {}
        override suspend fun syncNow() = Result.success(ListSyncReport(0, 0, 0, 0))
        override fun conflicts(): Flow<List<ListConflict>> = flowOf(emptyList())
        override suspend fun resolveConflict(id: String, choice: ConflictChoice) {}
        override suspend fun exportJson() = "[]"
        override suspend fun importJson(json: String) = Result.success(0)
    }

    private fun listed(progress: Int) = ListEntry(ref, "Seven", null, ListStatus.WATCHING, progress, 12, updatedAtMs = 1)

    private fun vm(history: History = History(emptyMap()), lists: Lists? = Lists(null)) =
        DetailsViewModel(MediaKind.ANIME, ref, Metadata(anime), history, Extensions(), Link(), lists = lists)

    @Test
    fun `list progress 2 of 12 resumes episode 3 instead of 1`() = runTest {
        val s = vm(lists = Lists(listed(2))).state.value
        assertEquals(3, s.continueEpisode)
        assertTrue(s.hasProgress, "Continuar, not Ver episodio 1")
        assertTrue(s.isWatched(1))
        assertTrue(s.isWatched(2))
        assertFalse(s.isWatched(3))
        assertEquals(3, s.panel?.episode)
    }

    @Test
    fun `the history position is used only when the history episode wins`() = runTest {
        // history: ep 2 at 10:00; list says 4 watched -> ep 5 from the start
        val history = History(mapOf(2 to h(2, 600, completed = false, at = 10)))
        val vm = vm(history, Lists(listed(4)))
        assertEquals(5, vm.state.value.continueEpisode)
        assertNull(vm.state.value.resumeEntry)
        vm.events.test {
            vm.onWatch()
            val event = assertIs<DetailsEvent.NavigateToPlayer>(awaitItem())
            assertEquals(5, event.args.episode)
            assertEquals(0L, event.args.startMs)
        }
        // history ahead of the list: its position resumes
        val ahead = vm(History(mapOf(6 to h(6, 300, completed = false, at = 10))), Lists(listed(2))).state.value
        assertEquals(6, ahead.continueEpisode)
        assertEquals(300, ahead.resumeEntry?.progressSec)
    }

    @Test
    fun `without the lists layer nothing changes`() = runTest {
        val s = vm(History(mapOf(3 to h(3, 100, completed = false, at = 10))), lists = null).state.value
        assertEquals(3, s.continueEpisode)
        assertEquals(100, s.resumeEntry?.progressSec)
        assertFalse(s.isWatched(1))
    }

    @Test
    fun `mark watched adds the title and raises the progress`() = runTest {
        val lists = Lists(null)
        val vm = vm(lists = lists)
        vm.toggleWatched(4)
        assertEquals(listOf("status:watching", "progress:4"), lists.calls)
        assertEquals(4, vm.state.value.listProgress)
        assertEquals(5, vm.state.value.continueEpisode)
        // episode 2 is watched now (2 <= 4): the toggle unmarks it, P = min(4, 1)
        vm.toggleWatched(2)
        assertEquals(listOf("status:watching", "progress:4", "progress:1"), lists.calls)
        assertEquals(2, vm.state.value.continueEpisode)
    }

    @Test
    fun `mark not watched lowers the progress and clears the history marks`() = runTest {
        val lists = Lists(listed(6))
        val history = History(mapOf(8 to h(8, 1350, completed = true, at = 10)))
        val vm = vm(history, lists)
        vm.toggleWatched(4)
        assertEquals(listOf("progress:3"), lists.calls)
        assertEquals(listOf(ref to 4), history.cleared)
        // above the list progress but completed in the history: only the history mark goes
        vm.toggleWatched(8)
        assertEquals(listOf("progress:3"), lists.calls)
        assertEquals(listOf(ref to 4, ref to 8), history.cleared)
    }

    @Test
    fun `unmarking an episode of a completed title reopens it as watching`() = runTest {
        val lists = Lists(ListEntry(ref, "Seven", null, ListStatus.COMPLETED, 12, 12, updatedAtMs = 1))
        val vm = vm(lists = lists)
        vm.toggleWatched(10)
        assertEquals(listOf("status:watching", "progress:9"), lists.calls)
        assertEquals(ListStatus.WATCHING, lists.row.value?.status)
        assertEquals(9, lists.row.value?.progress)
        assertEquals(10, vm.state.value.continueEpisode)
    }

    @Test
    fun `reaching the total completes the entry`() = runTest {
        val lists = Lists(listed(11))
        val vm = vm(lists = lists)
        vm.toggleWatched(12)
        assertEquals(ListStatus.COMPLETED, lists.row.value?.status)
        assertEquals(12, lists.row.value?.progress)
    }
}
