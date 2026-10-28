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

    private fun media(id: Int) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "Anime $id", titles = emptyMap(),
        cover = null, banner = null, color = null, synopsis = null, genres = emptyList(), episodes = null,
        status = null, season = null, year = null, score = null, sourceProvider = "fake",
    )

    private class FakeMetadata(val onSearch: (String, Int) -> FeedPage) : MetadataRepository {
        val queries = mutableListOf<String>()
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int) = FeedPage(emptyList(), page, false)
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
}
