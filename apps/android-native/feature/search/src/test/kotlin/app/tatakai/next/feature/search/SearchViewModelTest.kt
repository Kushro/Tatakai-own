package app.tatakai.next.feature.search

import app.cash.turbine.test
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs

@OptIn(ExperimentalCoroutinesApi::class)
class SearchViewModelTest {

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private fun media(id: Int, genres: List<String> = emptyList()) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "Anime $id", titles = emptyMap(),
        cover = null, banner = null, color = null, synopsis = null, genres = genres, episodes = null,
        status = null, season = null, year = null, score = null, sourceProvider = "fake",
    )

    private class FakeMetadata(
        val onFeed: (FeedKind, Int) -> FeedPage = { _, p -> FeedPage(emptyList(), p, false) },
        val onSearch: (String, Int) -> FeedPage,
    ) : MetadataRepository {
        val queries = mutableListOf<String>()
        val feeds = mutableListOf<String>()
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
            feeds += "$feed#$page"
            return onFeed(feed, page)
        }
        override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage {
            queries += "$query#$page"
            return onSearch(query, page)
        }
        override suspend fun media(kind: MediaKind, ref: MediaRef): Media? = null
        override suspend fun episodes(ref: MediaRef): List<Episode> = emptyList()
    }

    @Test
    fun `typing is debounced and only the last query hits the repository`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata { _, p -> FeedPage(listOf(media(1)), p, false) }
        val vm = SearchViewModel(repo, debounceMillis = 300)

        vm.onQueryChange("n")
        advanceTimeBy(100)
        vm.onQueryChange("naru")
        advanceTimeBy(100)
        vm.onQueryChange("naruto")
        advanceTimeBy(301)
        advanceUntilIdle()

        assertEquals(listOf("naruto#1"), repo.queries)
        assertIs<SearchResults.Ok>(vm.state.value.results)
    }

    @Test
    fun `states go idle - loading - empty - error and clear returns to idle`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata { q, p ->
            when (q) {
                "none" -> FeedPage(emptyList(), p, false)
                "boom" -> throw AppException(AppError.RateLimited("anilist"))
                else -> FeedPage(listOf(media(1)), p, false)
            }
        }
        val vm = SearchViewModel(repo, debounceMillis = 10)
        vm.state.test {
            assertEquals(SearchResults.Idle, awaitItem().results)
            vm.onQueryChange("none")
            advanceUntilIdle()
            var last = awaitItem()
            while (last.results != SearchResults.Empty) last = awaitItem()
            vm.onQueryChange("boom")
            advanceUntilIdle()
            while (last.results !is SearchResults.Err) last = awaitItem()
            assertEquals(AppError.RateLimited("anilist"), (last.results as SearchResults.Err).cause)
            vm.clear()
            advanceUntilIdle()
            while (last.results != SearchResults.Idle) last = awaitItem()
            cancelAndIgnoreRemainingEvents()
        }
    }

    @Test
    fun `load more appends the next page without duplicates`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata { _, p ->
            if (p == 1) FeedPage(listOf(media(1), media(2)), 1, true) else FeedPage(listOf(media(2), media(3)), 2, false)
        }
        val vm = SearchViewModel(repo, debounceMillis = 10)
        vm.onQueryChange("x")
        advanceUntilIdle()
        vm.loadMore()
        advanceUntilIdle()

        val ok = assertIs<SearchResults.Ok>(vm.state.value.results)
        assertEquals(listOf(1, 2, 3), ok.items.map { it.ref.anilistId })
        assertEquals(false, ok.hasNext)
    }

    @Test
    fun `submit records recent searches newest first, deduped and capped`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = InMemorySearchHistoryStore(listOf("b", "frieren"))
        val vm = SearchViewModel(FakeMetadata { _, p -> FeedPage(emptyList(), p, false) }, debounceMillis = 10, historyStore = store)

        assertEquals(listOf("b", "frieren"), vm.state.value.history)
        vm.onQueryChange(" frieren ")
        vm.submit()
        vm.onQueryChange("x") // too short: not recorded
        vm.submit()
        listOf("one", "two", "three", "four", "five").forEach {
            vm.onQueryChange(it)
            vm.submit()
        }
        advanceUntilIdle()

        val expected = listOf("five", "four", "three", "two", "one", "frieren")
        assertEquals(expected, vm.state.value.history)
        assertEquals(expected, store.load())

        vm.removeHistory("two")
        assertEquals(listOf("five", "four", "three", "one", "frieren"), store.load())
    }

    @Test
    fun `an initial feed browses that feed and pages it`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(
            onFeed = { _, p -> FeedPage(listOf(media(p * 10)), p, p == 1) },
            onSearch = { _, p -> FeedPage(emptyList(), p, false) },
        )
        val vm = SearchViewModel(repo, debounceMillis = 10)

        vm.applyInitial(query = null, feed = FeedKind.AIRING, genre = null)
        advanceUntilIdle()
        vm.loadMore()
        advanceUntilIdle()

        assertEquals(listOf("AIRING#1", "AIRING#2"), repo.feeds)
        assertEquals(emptyList(), repo.queries)
        assertEquals(listOf(10, 20), assertIs<SearchResults.Ok>(vm.state.value.results).items.map { it.ref.anilistId })

        // Applying the same initial values again (coming back to the screen) does not reload.
        vm.applyInitial(query = null, feed = FeedKind.AIRING, genre = null)
        advanceUntilIdle()
        assertEquals(2, repo.feeds.size)
    }

    @Test
    fun `an initial genre browses popular and filters on the client`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(
            onFeed = { _, p -> FeedPage(listOf(media(1, listOf("Action")), media(2, listOf("Romance"))), p, false) },
            onSearch = { _, p -> FeedPage(emptyList(), p, false) },
        )
        val vm = SearchViewModel(repo, debounceMillis = 10)

        vm.applyInitial(query = null, feed = null, genre = "action")
        advanceUntilIdle()

        assertEquals(listOf("POPULAR#1"), repo.feeds)
        assertEquals(listOf(1), vm.state.value.shown.map { it.ref.anilistId })

        vm.resetFilters()
        advanceUntilIdle()
        assertEquals(SearchResults.Idle, vm.state.value.results)
    }

    @Test
    fun `filters match genre, year, season and status`() {
        val m = media(1, listOf("Drama")).copy(year = 2024, season = "FALL", status = "RELEASING")
        assertEquals(true, SearchFilters(genre = "drama", year = 2024, season = "fall", status = "releasing").matches(m))
        assertEquals(false, SearchFilters(year = 2023).matches(m))
        assertEquals(2, SearchFilters(genre = "Drama", status = "FINISHED").activeCount)
    }
}
