package app.tatakai.next.feature.details

import app.cash.turbine.test
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.Container
import app.tatakai.next.model.DiscoveredPc
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
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.RemoteSession
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.WebInfo
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
        synopsis = null, genres = listOf("Action"), episodes = 12, status = null, season = null, year = null, score = null,
        sourceProvider = "fake",
    )
    private val webUrl = "https://cdn.example/a.m3u8"
    private val stream = StreamSource(webUrl, mapOf("Referer" to "https://x"), Container.HLS, "1080p", "prov", "ext")

    private val web = RankedSource(
        candidate = SourceCandidate(
            extId = "ext", extName = "Ext", kind = SourceKind.WEB, label = "Alpha",
            web = WebInfo(url = webUrl, headers = mapOf("Referer" to "https://x")),
        ),
        score = 90f,
        recommended = true,
    )
    private val hash = "0123456789abcdef0123456789abcdef01234567"
    private fun torrent(batch: Boolean = false, picked: Boolean = false) = RankedSource(
        candidate = SourceCandidate(
            extId = "nyaa", extName = "Nyaa", kind = SourceKind.TORRENT, label = "SubsPlease",
            torrent = TorrentInfo(infoHash = hash, seeders = 300),
            tags = ReleaseTags(batch = batch),
        ),
        score = 80f,
        picked = picked,
    )

    private fun entry(ep: Int, sec: Int, completed: Boolean, at: Long) = HistoryEntry(
        animeId = "7", title = "Seven", poster = null, episodeId = "7?ep=$ep", episode = ep, progressSec = sec,
        durationSec = 1400, completed = completed, watchedAt = Instant.ofEpochSecond(at), anilistId = 7, malId = null,
    )

    private class FakeMetadata(val media: Media?, val episodes: List<Episode>, val related: List<Media> = emptyList()) : MetadataRepository {
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int) = FeedPage(related, page, false)
        override suspend fun search(kind: MediaKind, query: String, page: Int) = FeedPage(emptyList(), page, false)
        override suspend fun media(kind: MediaKind, ref: MediaRef) = media
        override suspend fun episodes(ref: MediaRef) = episodes
    }

    private class FakeHistory(val progress: MutableStateFlow<Map<Int, HistoryEntry>>) : HistoryRepository {
        override fun continueWatching(limit: Int): Flow<List<HistoryEntry>> = emptyFlow()
        override suspend fun record(entry: HistoryEntry) {}
        override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> = progress
    }

    private class FakeExtensions(
        val candidates: (Int) -> List<RankedSource>,
        val streams: () -> List<StreamSource> = { emptyList() },
        enabled: Boolean = true,
    ) : ExtensionRepository {
        val resolved = mutableListOf<Int>()
        override val installed: StateFlow<List<InstalledExtension>> = MutableStateFlow(
            listOf(InstalledExtension("ext", "Ext", "1.0", "anime", emptyList(), ExtensionCompat.FULL, enabled)),
        )
        override suspend fun installFromUri(uri: String) = Result.failure<InstalledExtension>(UnsupportedOperationException())
        override suspend fun setEnabled(id: String, enabled: Boolean) {}
        override suspend fun remove(id: String) {}
        override suspend fun resolveStreams(media: Media, episode: Int) = streams()
        override suspend fun resolveCandidates(media: Media, episode: Int): List<RankedSource> {
            resolved += episode
            return candidates(episode)
        }
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

    private class FakeStats : SourceStatsStore {
        val picks = mutableListOf<Triple<String, Int, String>>()
        override suspend fun reliability(extId: String, label: String) = 0.8f
        override suspend fun recordSourceResult(extId: String, label: String, ok: Boolean) = 0.8f
        override suspend fun pickedSource(mediaId: String, episode: Int): String? = null
        override suspend fun setPickedSource(mediaId: String, episode: Int, key: String) {
            picks += Triple(mediaId, episode, key)
        }
        override suspend fun clearPickedSource(mediaId: String, episode: Int) {}
    }

    private val pc = PairedPc("d1", "PC", "192.168.1.5", 47321)
    private val session = RemoteSession("s1", 0, "https://pc/master.m3u8", null, 4, 60)

    private fun vm(
        progress: Map<Int, HistoryEntry> = emptyMap(),
        extensions: FakeExtensions = FakeExtensions({ listOf(web, torrent()) }, { listOf(stream) }),
        link: FakeLink = FakeLink(LinkState.Unpaired) { Result.success(session) },
        media: Media? = anime,
        torrents: FakeTorrents = FakeTorrents(),
        stats: FakeStats = FakeStats(),
        related: List<Media> = emptyList(),
    ) = DetailsViewModel(
        MediaKind.ANIME, ref,
        FakeMetadata(media, (1..12).map { Episode(it, "Ep $it", null, null) }, related),
        FakeHistory(MutableStateFlow(progress)), extensions, link,
        settings = null, torrents = torrents, stats = stats,
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
        assertNull(s.panel)
    }

    @Test
    fun `continue target resumes unfinished or advances after completed`() {
        assertEquals(1, continueTarget(emptyMap(), 12))
        assertEquals(3, continueTarget(mapOf(3 to entry(3, 100, false, 10)), 12))
        assertEquals(4, continueTarget(mapOf(3 to entry(3, 1300, true, 10)), 12))
        assertEquals(12, continueTarget(mapOf(12 to entry(12, 1300, true, 10)), 12))
    }

    @Test
    fun `the source panel resolves the continue episode once and keeps the recommended row`() = runTest {
        val ext = FakeExtensions({ listOf(web, torrent()) })
        val s = vm(progress = mapOf(3 to entry(3, 100, false, 10)), extensions = ext).state.value
        assertEquals(3, s.panel?.episode)
        assertEquals(listOf(3), ext.resolved)
        assertEquals(web, s.shownSource)
        assertNull(s.selectedKey)
        assertEquals(torrent(), s.bestTorrent)
    }

    @Test
    fun `a stored manual pick is selected and passed to the player`() = runTest {
        val picked = torrent(picked = true)
        val vm = vm(extensions = FakeExtensions({ listOf(web, picked) }))
        assertEquals(picked.key, vm.state.value.selectedKey)
        vm.events.test {
            vm.onWatch()
            val event = assertIs<DetailsEvent.NavigateToPlayer>(awaitItem())
            assertEquals(1, event.args.episode)
            assertEquals(picked.key, event.args.sourceKey)
        }
    }

    @Test
    fun `the split button picker plays the picked source and stores the pick`() = runTest {
        val stats = FakeStats()
        val vm = vm(stats = stats)
        vm.events.test {
            vm.openPicker(playOnPick = true)
            assertEquals(PickerRequest(1, playOnPick = true), vm.state.value.picker)
            vm.onPick(torrent())
            val event = assertIs<DetailsEvent.NavigateToPlayer>(awaitItem())
            assertEquals(torrent().key, event.args.sourceKey)
            assertNull(vm.state.value.picker)
            assertEquals(listOf(Triple("7", 1, torrent().key)), stats.picks)
        }
    }

    @Test
    fun `the panel picker only changes the source`() = runTest {
        val vm = vm()
        vm.events.test {
            vm.openPicker(playOnPick = false)
            vm.onPick(torrent())
            assertEquals(DetailsEvent.SourceChosen("Nyaa", "SubsPlease"), awaitItem())
            assertEquals(torrent().key, vm.state.value.selectedKey)
            assertEquals(torrent(), vm.state.value.shownSource)
        }
    }

    @Test
    fun `episode click navigates with the saved position`() = runTest {
        val vm = vm(progress = mapOf(2 to entry(2, 90, false, 5)))
        vm.events.test {
            vm.onEpisodeClick(5)
            val event = assertIs<DetailsEvent.NavigateToPlayer>(awaitItem())
            assertEquals(5, event.args.episode)
            assertEquals(0L, event.args.startMs)
            vm.onEpisodeClick(2)
            val resumed = assertIs<DetailsEvent.NavigateToPlayer>(awaitItem())
            assertEquals(90_000L, resumed.args.startMs)
            assertNull(resumed.args.remoteSessionId)
        }
    }

    @Test
    fun `candidate failure is shown in the panel and retry resolves again`() = runTest {
        var fail = true
        val ext = FakeExtensions({ if (fail) throw AppException(AppError.Extension("ext", "timeout")) else listOf(web) })
        val vm = vm(extensions = ext)
        assertEquals(AppError.Extension("ext", "timeout"), assertIs<LoadState.Err>(vm.state.value.panel?.sources).cause)
        fail = false
        vm.retrySources()
        assertEquals(listOf(web), assertIs<LoadState.Ok<List<RankedSource>>>(vm.state.value.panel?.sources).value)
    }

    @Test
    fun `no enabled extension is reported`() = runTest {
        val vm = vm(extensions = FakeExtensions({ emptyList() }, enabled = false))
        assertEquals(false, vm.state.value.hasExtensions)
    }

    @Test
    fun `download adds the torrent uri of the source and reports the id`() = runTest {
        val torrents = FakeTorrents { Result.success(hash) }
        val vm = vm(torrents = torrents)
        vm.events.test {
            vm.download(torrent(), episode = 1)
            assertEquals(DetailsEvent.DownloadQueued(hash), awaitItem())
        }
        assertEquals(listOf(FakeTorrents.Call("magnet:?xt=urn:btih:$hash", "Seven", 1)), torrents.downloads)
    }

    @Test
    fun `download failure and missing torrents are reported`() = runTest {
        val vm = vm(
            extensions = FakeExtensions({ ep -> if (ep == 1) listOf(web, torrent()) else listOf(web) }),
            torrents = FakeTorrents { Result.failure(AppException(AppError.Offline)) },
        )
        vm.events.test {
            vm.downloadEpisode(1)
            assertEquals(DetailsEvent.DownloadFailed(AppError.Offline), awaitItem())
            vm.downloadEpisode(4)
            assertEquals(DetailsEvent.NoTorrentSource, awaitItem())
            vm.downloadSeason() // best torrent is not a batch
            assertEquals(DetailsEvent.NoTorrentSource, awaitItem())
        }
    }

    @Test
    fun `a batch torrent offers the whole season`() = runTest {
        val torrents = FakeTorrents()
        val vm = vm(extensions = FakeExtensions({ listOf(torrent(batch = true)) }), torrents = torrents)
        assertTrue(vm.state.value.canDownloadSeason)
        vm.downloadSeason()
        assertEquals(listOf(FakeTorrents.Call("magnet:?xt=urn:btih:$hash", "Seven", null)), torrents.downloads)
    }

    @Test
    fun `send to device starts a url session with a web stream and navigates with its id`() = runTest {
        val link = FakeLink(LinkState.Paired(pc, reachable = true)) { Result.success(session) }
        val vm = vm(link = link)
        assertTrue(vm.state.value.pcAvailable)
        vm.events.test {
            vm.sendToDevice()
            val event = assertIs<DetailsEvent.NavigateToPlayer>(awaitItem())
            assertEquals("s1", event.args.remoteSessionId)
        }
        assertEquals(webUrl, assertIs<LinkSource.Url>(link.lastSource).url)
        assertEquals(false, vm.state.value.sending)
    }

    @Test
    fun `send failures are reported`() = runTest {
        val link = FakeLink(LinkState.Paired(pc, true)) { Result.failure(AppException(AppError.Offline)) }
        val vm = vm(link = link)
        vm.events.test {
            vm.sendToDevice()
            assertEquals(DetailsEvent.SendFailed(AppError.Offline), awaitItem())
        }
        val noWeb = vm(link = link, extensions = FakeExtensions({ listOf(torrent()) }))
        noWeb.events.test {
            noWeb.sendToDevice()
            assertEquals(DetailsEvent.NoWebSource, awaitItem())
        }
    }

    @Test
    fun `related lists other titles sharing a genre`() = runTest {
        val other = anime.copy(ref = MediaRef(anilistId = 8), title = "Eight")
        val unrelated = anime.copy(ref = MediaRef(anilistId = 9), title = "Nine", genres = listOf("Romance"))
        val vm = vm(related = listOf(anime, other, unrelated))
        assertNull(vm.state.value.related)
        vm.loadRelated()
        assertEquals(listOf(8), assertIs<LoadState.Ok<List<Media>>>(vm.state.value.related).value.map { it.ref.anilistId })
    }
}
