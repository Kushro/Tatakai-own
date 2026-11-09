// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import app.tatakai.next.model.AutoDownloadPreview
import app.tatakai.next.model.AutoDownloadRule
import app.tatakai.next.model.ChapterDownload
import app.tatakai.next.model.ChapterDownloadSource
import app.tatakai.next.model.ChapterKind
import app.tatakai.next.model.Container
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadProfile
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.DownloadRequest
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.DownloadsSummary
import app.tatakai.next.model.DownloadsTab
import app.tatakai.next.model.LocalLibraryItem
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import java.io.IOException
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

// F6 (parity wave B tests, 16 §5 r.5): DownloadsViewModel over a fake DownloadRepository and fake ChapterDownloadSources, injected clock.
@OptIn(ExperimentalCoroutinesApi::class)
class DownloadsViewModelTest {
    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private fun item(id: Int, ep: Int, state: DownloadState, progress: Double = 0.0, title: String = "Show $id", done: Long? = null) = DownloadItem(
        id = "$id?ep=$ep", ref = MediaRef(anilistId = id), title = title, episode = ep, extId = "e", sourceLabel = "Alpha",
        container = Container.HLS, state = state, progress = progress, totalBytes = 100, createdAtMs = ep.toLong(), completedAtMs = done,
    )

    private fun chapter(id: String, kind: ChapterKind, title: String) = ChapterDownload(
        id = id, kind = kind, mediaId = "m", title = title, poster = null, chapterLabel = "Ch. 1", chapterNumber = 1.0,
        state = DownloadState.COMPLETED, createdAtMs = 0, localRef = "{}",
    )

    private class FakeDownloads : DownloadRepository {
        override val items = MutableStateFlow<List<DownloadItem>>(emptyList())
        override val summary = MutableStateFlow(DownloadsSummary())
        override val rules = MutableStateFlow<List<AutoDownloadRule>>(emptyList())
        override val library = MutableStateFlow<List<LocalLibraryItem>>(emptyList())
        val historyFlow = MutableStateFlow<List<DownloadItem>>(emptyList())
        val removed = mutableListOf<String>()
        val paused = mutableListOf<String>()
        var used = 1234L
        var usageCalls = 0
        var checkResult: Result<Int> = Result.success(2)
        var broken: List<DownloadItem> = emptyList()
        var importResult: Result<List<LocalLibraryItem>> = Result.success(emptyList())
        var pauseError: Exception? = null

        override suspend fun enqueue(request: DownloadRequest): Result<DownloadItem> = Result.failure(UnsupportedOperationException())
        override suspend fun enqueueSeason(media: Media, episodes: List<Int>, preferredServer: String?, preferredAudioLang: String?) =
            Result.success(emptyList<DownloadItem>())
        override suspend fun pause(id: String) {
            pauseError?.let { throw it }
            paused += id
        }
        override suspend fun resume(id: String) {}
        override suspend fun remove(id: String) {
            removed += id
        }
        override suspend fun retry(id: String) {}
        override fun history(limit: Int): Flow<List<DownloadItem>> = historyFlow
        override suspend fun clearHistory() {
            historyFlow.value = emptyList()
        }
        override suspend fun localUriFor(ref: MediaRef, episode: Int): String? = null
        override fun downloadedEpisodes(ref: MediaRef): Flow<Map<Int, DownloadItem>> = MutableStateFlow(emptyMap())
        override suspend fun setRule(rule: AutoDownloadRule?) {
            if (rule != null) rules.value = rules.value.filterNot { it.ref == rule.ref } + rule
        }
        override suspend fun preview(ref: MediaRef): AutoDownloadPreview? = null
        override suspend fun checkNow(): Result<Int> = checkResult
        override suspend fun importFiles(uris: List<String>) = importResult
        override suspend fun scanFolder(treeUri: String) = importResult
        override suspend fun matchLocal(id: String, media: Media?, episode: Int?) {}
        override suspend fun removeLocal(id: String, deleteFile: Boolean) {}
        override suspend fun storageUsedBytes(): Long {
            usageCalls++
            return used
        }
        override suspend fun verifyIntegrity(): List<DownloadItem> = broken
    }

    private class FakeChapters(override val kind: ChapterKind, initial: List<ChapterDownload>, val failing: Boolean = false) : ChapterDownloadSource {
        val rows = MutableStateFlow(initial)
        val removed = mutableListOf<String>()
        override fun downloads(): Flow<List<ChapterDownload>> = if (failing) flow<List<ChapterDownload>> { throw IOException("db") } else rows
        override suspend fun remove(id: String) {
            removed += id
        }
        override suspend fun retry(id: String) {}
    }

    private fun TestScope.subscribe(vm: DownloadsViewModel): MutableList<DownloadsEvent> {
        val events = mutableListOf<DownloadsEvent>()
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.state.collect {} }
        backgroundScope.launch(UnconfinedTestDispatcher(testScheduler)) { vm.events.collect { events += it } }
        return events
    }

    @Test
    fun tabsSplitWebChaptersAndHistoryAndSearchFiltersThem() = runTest {
        val repo = FakeDownloads()
        repo.items.value = listOf(
            item(1, 1, DownloadState.DOWNLOADING, title = "Frieren"),
            item(1, 2, DownloadState.COMPLETED, title = "Frieren", done = 5),
            item(2, 1, DownloadState.COMPLETED, title = "Berserk", done = 9),
        )
        repo.historyFlow.value = listOf(item(3, 1, DownloadState.COMPLETED, title = "Old", done = 1))
        val manga = FakeChapters(ChapterKind.MANGA, listOf(chapter("m1", ChapterKind.MANGA, "Berserk")))
        val novel = FakeChapters(ChapterKind.NOVEL, listOf(chapter("n1", ChapterKind.NOVEL, "Mushoku")))
        val vm = DownloadsViewModel(repo, torrents = null, chapters = listOf(manga, novel), clock = { 0L })
        subscribe(vm)

        val s = vm.state.value
        assertTrue(s.loaded)
        assertEquals(listOf("1?ep=1"), s.active.map { it.id })
        assertEquals(listOf("Berserk", "Frieren"), s.anime.map { it.title }, "groups sorted by last completion")
        assertEquals(listOf("m1"), s.manga.map { it.id })
        assertEquals(listOf("n1"), s.novels.map { it.id })
        assertEquals(1, s.history.size)
        assertEquals(1234L, s.usedBytes)

        vm.setQuery("berserk")
        val f = vm.state.value
        assertTrue(f.active.isEmpty())
        assertEquals(listOf("Berserk"), f.anime.map { it.title })
        assertEquals(listOf("m1"), f.manga.map { it.id })
        assertTrue(f.novels.isEmpty())

        vm.selectTab(DownloadsTab.MANGA)
        assertEquals(DownloadsTab.MANGA, vm.state.value.tab)
    }

    @Test
    fun failingChapterSourceDoesNotHideTheOthers() = runTest {
        val repo = FakeDownloads()
        val broken = FakeChapters(ChapterKind.NOVEL, emptyList(), failing = true)
        val manga = FakeChapters(ChapterKind.MANGA, listOf(chapter("m1", ChapterKind.MANGA, "X")))
        val vm = DownloadsViewModel(repo, null, listOf(broken, manga), clock = { 0L })
        subscribe(vm)
        assertEquals(listOf("m1"), vm.state.value.manga.map { it.id })
        assertTrue(vm.state.value.novels.isEmpty())
        vm.removeChapter(vm.state.value.manga.single())
        assertEquals(listOf("m1"), manga.removed)
    }

    @Test
    fun etaComesFromTwoProgressSamples() = runTest {
        var now = 0L
        val repo = FakeDownloads()
        repo.items.value = listOf(item(1, 1, DownloadState.DOWNLOADING, progress = 0.25))
        val vm = DownloadsViewModel(repo, clock = { now })
        subscribe(vm)
        assertTrue(vm.state.value.eta.isEmpty(), "one sample is not enough")
        now = 10_000L
        repo.items.value = listOf(item(1, 1, DownloadState.DOWNLOADING, progress = 0.75))
        assertEquals(5L, vm.state.value.eta["1?ep=1"])
        repo.items.value = listOf(item(1, 1, DownloadState.PAUSED, progress = 0.75))
        assertTrue(vm.state.value.eta.isEmpty())
    }

    @Test
    fun removeTitleRemovesEveryEpisodeAndRefreshesUsage() = runTest {
        val repo = FakeDownloads()
        repo.items.value = listOf(item(1, 1, DownloadState.COMPLETED, done = 1), item(1, 2, DownloadState.COMPLETED, done = 2))
        val vm = DownloadsViewModel(repo, clock = { 0L })
        subscribe(vm)
        val calls = repo.usageCalls
        repo.used = 0L
        vm.removeTitle(vm.state.value.anime.single())
        assertEquals(listOf("1?ep=1", "1?ep=2"), repo.removed)
        assertTrue(repo.usageCalls > calls)
        assertEquals(0L, vm.state.value.usedBytes)
    }

    @Test
    fun checkVerifyAndImportEmitEvents() = runTest {
        val repo = FakeDownloads()
        val vm = DownloadsViewModel(repo, clock = { 0L })
        val events = subscribe(vm)

        vm.checkNow()
        assertEquals(DownloadsEvent.Checked(2), events.last())
        assertFalse(vm.state.value.busy)

        repo.broken = listOf(item(1, 1, DownloadState.BROKEN))
        vm.verify()
        assertEquals(DownloadsEvent.Verified(1), events.last())

        val err = IOException("saf")
        repo.importResult = Result.failure(err)
        vm.importFiles(listOf("content://x"))
        assertEquals(err, assertIs<DownloadsEvent.Failed>(events.last()).cause)

        repo.checkResult = Result.failure(IllegalStateException("no rules"))
        vm.checkNow()
        assertIs<DownloadsEvent.Failed>(events.last())
        assertFalse(vm.state.value.busy)
    }

    @Test
    fun repositoryExceptionsBecomeFailedEvents() = runTest {
        val repo = FakeDownloads()
        repo.pauseError = IOException("gone")
        val vm = DownloadsViewModel(repo, clock = { 0L })
        val events = subscribe(vm)
        vm.pause("1?ep=1")
        assertIs<DownloadsEvent.Failed>(events.single())
    }

    @Test
    fun addRuleUsesTheChosenProfile() = runTest {
        val repo = FakeDownloads()
        val vm = DownloadsViewModel(repo, clock = { 0L })
        subscribe(vm)
        vm.addRule(MediaRef(anilistId = 5), "Show", null, DownloadProfile.SAVER_720)
        val rule = vm.state.value.rules.single()
        assertEquals(MediaRef(anilistId = 5), rule.ref)
        assertEquals(DownloadProfile.SAVER_720, rule.profile)
    }
}
