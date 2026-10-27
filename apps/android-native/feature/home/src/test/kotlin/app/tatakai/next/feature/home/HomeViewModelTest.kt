package app.tatakai.next.feature.home

import app.cash.turbine.test
import app.tatakai.next.model.AppError
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import java.io.IOException
import java.time.Instant
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
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
class HomeViewModelTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private fun media(id: Int) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "Anime $id", titles = emptyMap(),
        cover = null, banner = null, color = null, synopsis = null, genres = emptyList(), episodes = 12,
        status = null, season = null, year = 2024, score = 80, sourceProvider = "fake",
    )

    private class FakeMetadata(
        val feeds: MutableMap<FeedKind, () -> FeedPage>,
    ) : MetadataRepository {
        var calls = 0
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
            calls++
            return feeds.getValue(feed)()
        }
        override suspend fun search(kind: MediaKind, query: String, page: Int) = FeedPage(emptyList(), page, false)
        override suspend fun media(kind: MediaKind, ref: MediaRef): Media? = null
        override suspend fun episodes(ref: MediaRef): List<Episode> = emptyList()
    }

    private class FakeHistory(initial: List<HistoryEntry> = emptyList()) : HistoryRepository {
        val entries = MutableStateFlow(initial)
        override fun continueWatching(limit: Int): Flow<List<HistoryEntry>> = entries
        override suspend fun record(entry: HistoryEntry) {}
        override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> = MutableStateFlow(emptyMap())
    }

    private fun entry(ep: Int) = HistoryEntry(
        animeId = "1", title = "Anime 1", poster = null, episodeId = "1?ep=$ep", episode = ep, progressSec = 60,
        durationSec = 1400, completed = false, watchedAt = Instant.EPOCH, anilistId = 1, malId = null,
    )

    @Test
    fun `every feed loads independently and the hero keeps five items`() = runTest {
        val metadata = FakeMetadata(
            HOME_FEEDS.associateWith { kind ->
                { FeedPage((1..8).map { media(it + kind.ordinal * 100) }, 1, true) }
            }.toMutableMap(),
        )
        val vm = HomeViewModel(metadata, FakeHistory())

        val state = vm.state.first { s -> s.feeds.none { it.state is LoadState.Loading } }

        assertEquals(HOME_FEEDS.size, metadata.calls)
        val hero = assertIs<LoadState.Ok<List<Media>>>(state.hero)
        assertEquals(HERO_SIZE, hero.value.size)
        assertTrue(state.rows.none { it.kind == HERO_FEED })
        assertTrue(state.rows.all { it.state is LoadState.Ok })
    }

    @Test
    fun `a failing row does not break the others and retry reloads only that row`() = runTest {
        var airingFails = true
        val feeds = HOME_FEEDS.associateWith { kind ->
            {
                if (kind == FeedKind.AIRING && airingFails) throw IOException("offline")
                FeedPage(listOf(media(kind.ordinal + 1)), 1, false)
            }
        }.toMutableMap()
        val metadata = FakeMetadata(feeds)
        val vm = HomeViewModel(metadata, FakeHistory())

        val failed = vm.state.first { s -> s.feeds.none { it.state is LoadState.Loading } }
        val airing = failed.feeds.first { it.kind == FeedKind.AIRING }.state
        assertEquals(AppError.Offline, assertIs<LoadState.Err>(airing).cause)
        assertTrue(failed.feeds.filter { it.kind != FeedKind.AIRING }.all { it.state is LoadState.Ok })

        airingFails = false
        val callsBefore = metadata.calls
        vm.retry(FeedKind.AIRING)

        val recovered = vm.state.first { s -> s.feeds.first { it.kind == FeedKind.AIRING }.state is LoadState.Ok }
        assertEquals(callsBefore + 1, metadata.calls)
        assertIs<LoadState.Ok<List<Media>>>(recovered.feeds.first { it.kind == FeedKind.AIRING }.state)
    }

    @Test
    fun `continue watching follows the history flow`() = runTest {
        val history = FakeHistory()
        val metadata = FakeMetadata(HOME_FEEDS.associateWith { { FeedPage(emptyList(), 1, false) } }.toMutableMap())
        val vm = HomeViewModel(metadata, history)

        vm.state.test {
            assertTrue(awaitUntil { it.continueWatching.isEmpty() && it.feeds.none { r -> r.state is LoadState.Loading } })
            history.entries.value = listOf(entry(3))
            assertTrue(awaitUntil { it.continueWatching.map { e -> e.episode } == listOf(3) })
            cancelAndIgnoreRemainingEvents()
        }
    }

    private suspend fun app.cash.turbine.ReceiveTurbine<HomeUiState>.awaitUntil(predicate: (HomeUiState) -> Boolean): Boolean {
        while (true) {
            val item = awaitItem()
            if (predicate(item)) return true
        }
    }
}
