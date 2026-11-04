// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents

import app.tatakai.next.feature.torrents.vm.PeerSort
import app.tatakai.next.feature.torrents.vm.TorrentDetailViewModel
import app.tatakai.next.feature.torrents.vm.TorrentDownloadsViewModel
import app.tatakai.next.feature.torrents.vm.TorrentFieldRules
import app.tatakai.next.feature.torrents.vm.TorrentLibraryViewModel
import app.tatakai.next.feature.torrents.vm.TorrentMessage
import app.tatakai.next.feature.torrents.vm.TorrentSettingsViewModel
import app.tatakai.next.feature.torrents.vm.downloadsOf
import app.tatakai.next.feature.torrents.vm.filterFiles
import app.tatakai.next.feature.torrents.vm.sortPeers
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.torrent.TorrentEvent
import app.tatakai.next.model.torrent.TorrentFile
import app.tatakai.next.model.torrent.TorrentId
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentPeer
import app.tatakai.next.model.torrent.TorrentPlayback
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentServiceStatus
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.model.torrent.TorrentStats
import app.tatakai.next.model.torrent.TorrentStorage
import app.tatakai.next.model.torrent.TorrentTracker
import app.tatakai.next.model.torrent.StorageLocation
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.flowOf
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
class TorrentViewModelsTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private fun item(id: String, progress: Double = 0.5, persist: Boolean = true) = TorrentLibraryItem(
        id = id, name = "[G] Show - 01.mkv", series = "Show", episode = 1, progress = progress, persist = persist,
        state = TorrentState.DOWNLOADING, addedUnix = 1,
    )

    private class FakeTorrents : TorrentRepository {
        override val events = MutableSharedFlow<TorrentEvent>()
        override val library = MutableStateFlow<List<TorrentLibraryItem>>(emptyList())
        override val status = MutableStateFlow(TorrentServiceStatus())
        val calls = mutableListOf<String>()
        var statsFlow: Flow<TorrentStats> = flowOf()
        var filesResult: () -> List<TorrentFile> = { emptyList() }

        override suspend fun play(source: String, media: Media, episode: Int, fileIndex: Int?): Result<TorrentPlayback> =
            Result.failure(AppException(AppError.Offline))
        override suspend fun playbackEnded(torrentId: TorrentId, completed: Boolean) {}
        override suspend fun addDownload(source: String, media: Media, episode: Int?): Result<TorrentId> = Result.success("x")
        override fun stats(id: TorrentId): Flow<TorrentStats> = statsFlow
        override suspend fun files(id: TorrentId): List<TorrentFile> = filesResult()
        override fun peers(id: TorrentId): Flow<List<TorrentPeer>> = flowOf(emptyList())
        override suspend fun trackers(id: TorrentId): List<TorrentTracker> = emptyList()
        override suspend fun remove(id: TorrentId, deleteFiles: Boolean) {
            calls += "remove:$id:$deleteFiles"
            library.value = library.value.filterNot { it.id == id }
        }
        override suspend fun pause(id: TorrentId) { calls += "pause:$id" }
        override suspend fun resume(id: TorrentId) { calls += "resume:$id" }
        override suspend fun setPersist(id: TorrentId, persist: Boolean) { calls += "persist:$id:$persist" }
        override suspend fun setSeeding(id: TorrentId, seed: Boolean) { calls += "seed:$id:$seed" }
        override suspend fun downloadFile(id: TorrentId, fileIndex: Int) { calls += "file:$id:$fileIndex" }
        override suspend fun seekMarks(id: TorrentId, fileIndex: Int): List<ClosedFloatingPointRange<Double>> = emptyList()
        override suspend fun exportToDownloads(id: TorrentId, fileIndex: Int): Result<String> = Result.success("content://x")
        override suspend fun storageUsage(): TorrentStorage = TorrentStorage(10, 20, StorageLocation.AppSpecific)
    }

    private class FakeSettings : SettingsRepository {
        override val settings = MutableStateFlow(AppSettings())
        override suspend fun update(transform: (AppSettings) -> AppSettings) {
            settings.value = transform(settings.value)
        }
    }

    @Test
    fun `library exposes items and empty state`() = runTest {
        val repo = FakeTorrents()
        val vm = TorrentLibraryViewModel(repo)
        assertEquals(emptyList(), assertIs<LoadState.Ok<List<TorrentLibraryItem>>>(vm.state.first { it !is LoadState.Loading }).value)
        repo.library.value = listOf(item("a"), item("b"))
        assertEquals(listOf("a", "b"), assertIs<LoadState.Ok<List<TorrentLibraryItem>>>(vm.state.first { it is LoadState.Ok && it.value.isNotEmpty() }).value.map { it.id })
    }

    @Test
    fun `library multi-select and bulk delete`() = runTest {
        val repo = FakeTorrents()
        repo.library.value = listOf(item("a"), item("b"))
        val vm = TorrentLibraryViewModel(repo)
        vm.toggleSelection("a")
        vm.toggleSelection("b")
        vm.toggleSelection("b")
        assertEquals(setOf("a"), vm.selection.value)
        vm.selectAll(listOf("a", "b"))
        assertEquals(setOf("a", "b"), vm.selection.value)
        vm.remove(listOf("a", "b"), deleteFiles = false)
        assertEquals(listOf("remove:a:false", "remove:b:false"), repo.calls)
        assertTrue(vm.selection.value.isEmpty())
        vm.pause(listOf("x"))
        vm.setPersist(listOf("x"), true)
        assertEquals(listOf("pause:x", "persist:x:true"), repo.calls.drop(2))
    }

    @Test
    fun `detail stats error is surfaced and retry recovers`() = runTest {
        val repo = FakeTorrents()
        repo.statsFlow = flow { throw AppException(AppError.Unknown("torrent:missing:a")) }
        val vm = TorrentDetailViewModel("a", repo)
        val err = assertIs<LoadState.Err>(vm.stats.first { it !is LoadState.Loading })
        assertEquals(AppError.Unknown("torrent:missing:a"), err.cause)
        val ok = TorrentStats(id = "a", name = "n", infoHash = "a", state = TorrentState.DOWNLOADING)
        repo.statsFlow = flowOf(ok)
        vm.retryStats()
        assertEquals(ok, assertIs<LoadState.Ok<TorrentStats>>(vm.stats.first { it is LoadState.Ok }).value)
    }

    @Test
    fun `detail remove signals navigation and download file calls the repository`() = runTest {
        val repo = FakeTorrents()
        repo.library.value = listOf(item("a"))
        val vm = TorrentDetailViewModel("a", repo)
        assertEquals("a", vm.item.value?.id)
        vm.downloadFile(3)
        vm.setSeeding(true)
        vm.remove(deleteFiles = true)
        assertEquals(listOf("file:a:3", "seed:a:true", "remove:a:true"), repo.calls)
        assertTrue(vm.removed.value)
    }

    @Test
    fun `downloads filters unfinished persisted torrents and the ack enables background`() = runTest {
        val repo = FakeTorrents()
        val settings = FakeSettings()
        repo.library.value = listOf(item("a"), item("done", progress = 1.0), item("play", persist = false))
        val vm = TorrentDownloadsViewModel(repo, settings)
        val ui = assertIs<LoadState.Ok<*>>(vm.state.first { it is LoadState.Ok }).value as app.tatakai.next.feature.torrents.vm.DownloadsUi
        assertEquals(listOf("a"), ui.items.map { it.id })
        vm.acknowledge()
        assertTrue(settings.settings.value.torrent.backgroundAck)
        assertTrue(settings.settings.value.torrent.backgroundDownloads)
        vm.setBackground(false)
        assertEquals(false, settings.settings.value.torrent.backgroundDownloads)
    }

    @Test
    fun `settings updates are sanitized and saved`() = runTest {
        val settings = FakeSettings()
        val vm = TorrentSettingsViewModel(settings, FakeTorrents())
        vm.update { it.copy(maxPeers = 9999, listenPort = 80) }
        assertEquals(500, settings.settings.value.torrent.maxPeers)
        assertEquals(0, settings.settings.value.torrent.listenPort)
        assertEquals(TorrentStorage(10, 20, StorageLocation.AppSpecific), assertIs<LoadState.Ok<TorrentStorage>>(vm.storage.first { it is LoadState.Ok }).value)
    }

    @Test
    fun `field rules`() {
        assertEquals(0, TorrentFieldRules.port("0"))
        assertEquals(6881, TorrentFieldRules.port(" 6881 "))
        assertNull(TorrentFieldRules.port("80"))
        assertNull(TorrentFieldRules.port("70000"))
        assertEquals(0, TorrentFieldRules.limit("0"))
        assertNull(TorrentFieldRules.limit("1000001"))
        assertNull(TorrentFieldRules.peers("0"))
        assertEquals(500, TorrentFieldRules.peers("500"))
        assertNull(TorrentFieldRules.peers("abc"))
    }

    @Test
    fun `peer sorting never loses rows and file search matches folders`() {
        val peers = listOf(
            TorrentPeer(addr = "1", downBps = 5, upBps = 1, progress = 0.2),
            TorrentPeer(addr = "2", downBps = 9, upBps = 0, progress = 0.9),
            TorrentPeer(addr = "3", downBps = 1, upBps = 7, progress = 0.5),
        )
        assertEquals(listOf("2", "1", "3"), sortPeers(peers, PeerSort.DOWN).map { it.addr })
        assertEquals(listOf("3", "1", "2"), sortPeers(peers, PeerSort.UP).map { it.addr })
        assertEquals(listOf("2", "3", "1"), sortPeers(peers, PeerSort.PROGRESS).map { it.addr })

        val files = listOf(
            TorrentFile(index = 0, path = "Show S1/Show - 01.mkv", name = "Show - 01.mkv", sizeBytes = 1),
            TorrentFile(index = 1, path = "Extras/NCOP.mkv", name = "NCOP.mkv", sizeBytes = 1),
        )
        assertEquals(listOf(1), filterFiles(files, "extras").map { it.index })
        assertEquals(2, filterFiles(files, " ").size)
        assertEquals(listOf("a"), downloadsOf(listOf(item("a"), item("b", progress = 1.0))).map { it.id })
    }

    @Test
    fun `messages type is exhaustive for the snackbar`() {
        val all: List<TorrentMessage> = listOf(TorrentMessage.Removed(1), TorrentMessage.Saved, TorrentMessage.Exported("u"), TorrentMessage.Failed(AppError.Offline))
        assertEquals(4, all.size)
    }
}
