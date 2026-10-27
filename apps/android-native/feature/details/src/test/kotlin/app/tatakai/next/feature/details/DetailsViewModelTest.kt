package app.tatakai.next.feature.details

import app.cash.turbine.test
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.Container
import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.Episode
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
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.RemoteSession
import app.tatakai.next.model.StreamSource
import java.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class DetailsViewModelTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private val ref = MediaRef(anilistId = 7)
    private val anime = Media(
        ref = ref, kind = MediaKind.ANIME, title = "Seven", titles = emptyMap(), cover = null, banner = null, color = null,
        synopsis = null, genres = emptyList(), episodes = 12, status = null, season = null, year = null, score = null,
        sourceProvider = "fake",
    )
    private val source = StreamSource("https://cdn.example/a.m3u8", mapOf("Referer" to "https://x"), Container.HLS, "1080p", "prov", "ext")

    private fun entry(ep: Int, sec: Int, completed: Boolean, at: Long) = HistoryEntry(
        animeId = "7", title = "Seven", poster = null, episodeId = "7?ep=$ep", episode = ep, progressSec = sec,
        durationSec = 1400, completed = completed, watchedAt = Instant.ofEpochSecond(at), anilistId = 7, malId = null,
    )

    private class FakeMetadata(val media: Media?, val episodes: List<Episode>) : MetadataRepository {
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int) = FeedPage(emptyList(), page, false)
        override suspend fun search(kind: MediaKind, query: String, page: Int) = FeedPage(emptyList(), page, false)
        override suspend fun media(kind: MediaKind, ref: MediaRef) = media
        override suspend fun episodes(ref: MediaRef) = episodes
    }

    private class FakeHistory(val progress: MutableStateFlow<Map<Int, HistoryEntry>>) : HistoryRepository {
        override fun continueWatching(limit: Int): Flow<List<HistoryEntry>> = emptyFlow()
        override suspend fun record(entry: HistoryEntry) {}
        override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> = progress
    }

    private class FakeExtensions(val streams: () -> List<StreamSource>) : ExtensionRepository {
        override val installed: StateFlow<List<InstalledExtension>> = MutableStateFlow(emptyList())
        override suspend fun installFromUri(uri: String) = Result.failure<InstalledExtension>(UnsupportedOperationException())
        override suspend fun setEnabled(id: String, enabled: Boolean) {}
        override suspend fun remove(id: String) {}
        override suspend fun resolveStreams(media: Media, episode: Int) = streams()
    }

    private class FakeLink(initial: LinkState, val result: () -> Result<RemoteSession>) : LinkRepository {
        override val state = MutableStateFlow(initial)
        var lastSource: LinkSource? = null
        override fun discover(): Flow<List<DiscoveredPc>> = emptyFlow()
        override suspend fun pair(payload: String, deviceName: String) = Result.failure<PairedPc>(UnsupportedOperationException())
        override suspend fun capabilities() = Result.failure<LinkCapabilities>(UnsupportedOperationException())
        override suspend fun startSession(source: LinkSource, profileId: String?): Result<RemoteSession> {
            lastSource = source
            return result()
        }
        override suspend fun stop(sessionId: String) {}
        override suspend fun forget() {}
    }

    private val pc = PairedPc("d1", "PC", "192.168.1.5", 47321)
    private val session = RemoteSession("s1", 0, "https://pc/master.m3u8", null, 4, 60)

    private fun vm(
        progress: Map<Int, HistoryEntry> = emptyMap(),
        streams: () -> List<StreamSource> = { listOf(source) },
        link: FakeLink = FakeLink(LinkState.Unpaired) { Result.success(session) },
        media: Media? = anime,
    ) = DetailsViewModel(
        MediaKind.ANIME, ref,
        FakeMetadata(media, (1..12).map { Episode(it, "Ep $it", null, null) }),
        FakeHistory(MutableStateFlow(progress)), FakeExtensions(streams), link,
    )

    @Test
    fun `loads media and sorted episodes`() = runTest {
        val s = vm().state.value
        assertEquals("Seven", assertIs<LoadState.Ok<Media>>(s.media).value.title)
        assertEquals(12, assertIs<LoadState.Ok<List<Episode>>>(s.episodes).value.size)
    }

    @Test
    fun `missing media becomes a 404 error`() = runTest {
        val s = vm(media = null).state.value
        assertEquals(AppError.Http(404), assertIs<LoadState.Err>(s.media).cause)
    }

    @Test
    fun `continue target resumes unfinished or advances after completed`() {
        assertEquals(1, continueTarget(emptyMap(), 12))
        assertEquals(3, continueTarget(mapOf(3 to entry(3, 100, false, 10)), 12))
        assertEquals(4, continueTarget(mapOf(3 to entry(3, 1300, true, 10)), 12))
        assertEquals(12, continueTarget(mapOf(12 to entry(12, 1300, true, 10)), 12))
    }

    @Test
    fun `episode click resolves sources and play navigates with the saved position`() = runTest {
        val vm = vm(progress = mapOf(2 to entry(2, 90, false, 5)))
        vm.events.test {
            vm.onEpisodeClick(2)
            val sheet = openSheet(vm)
            assertEquals(listOf(source), assertIs<LoadState.Ok<List<StreamSource>>>(sheet.sources).value)
            vm.onPlay()
            val event = assertIs<DetailsEvent.NavigateToPlayer>(awaitItem())
            assertEquals(2, event.args.episode)
            assertEquals(90_000L, event.args.startMs)
            assertNull(event.args.remoteSessionId)
            assertNull(vm.state.value.sheet)
        }
    }

    @Test
    fun `source failure is reported in the sheet`() = runTest {
        val vm = vm(streams = { throw AppException(AppError.Extension("ext", "timeout")) })
        vm.onEpisodeClick(1)
        assertEquals(AppError.Extension("ext", "timeout"), assertIs<LoadState.Err>(openSheet(vm).sources).cause)
    }

    @Test
    fun `enhance on pc starts a url session and navigates with its id`() = runTest {
        val link = FakeLink(LinkState.Paired(pc, reachable = true)) { Result.success(session) }
        val vm = vm(link = link)
        assertTrue(vm.state.value.pcAvailable)
        vm.events.test {
            vm.onEpisodeClick(1)
            vm.onPlayOnPc(source)
            val event = assertIs<DetailsEvent.NavigateToPlayer>(awaitItem())
            assertEquals("s1", event.args.remoteSessionId)
            assertEquals(LinkSource.Url(source.url, source.headers), link.lastSource)
        }
    }

    @Test
    fun `pc failure keeps the sheet open with the error`() = runTest {
        val link = FakeLink(LinkState.Paired(pc, true)) { Result.failure(AppException(AppError.Offline)) }
        val vm = vm(link = link)
        vm.onEpisodeClick(1)
        vm.onPlayOnPc(source)
        val sheet = openSheet(vm)
        assertEquals(AppError.Offline, sheet.remoteError)
        assertEquals(false, sheet.startingRemote)
    }

    private fun openSheet(vm: DetailsViewModel): SourceSheet = checkNotNull(vm.state.value.sheet) { "sheet should be open" }
}
