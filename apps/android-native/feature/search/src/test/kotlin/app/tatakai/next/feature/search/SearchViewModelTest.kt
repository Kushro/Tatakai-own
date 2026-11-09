package app.tatakai.next.feature.search

import app.cash.turbine.test
import app.tatakai.next.metadata.TrendWindow
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.ImageMatch
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.MediaTag
import app.tatakai.next.model.MetadataRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import java.time.LocalDate
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class SearchViewModelTest {

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private fun media(id: Int, genres: List<String> = emptyList()) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "Anime $id", titles = emptyMap(),
        cover = null, banner = null, color = null, synopsis = null, genres = genres, episodes = null,
        status = null, season = null, year = null, score = null, sourceProvider = "fake",
    )

    /** Records every MediaSearchQuery (server-side filters) and every feed / character call. */
    private class FakeMetadata(
        val onFeed: (FeedKind, Int) -> FeedPage = { _, p -> FeedPage(emptyList(), p, false) },
        val onAdvanced: (MediaSearchQuery, Int) -> FeedPage,
        val onCharacters: (String, Int) -> List<CharacterRef> = { _, _ -> emptyList() },
    ) : MetadataRepository {
        val advanced = mutableListOf<Pair<MediaSearchQuery, Int>>()
        val feeds = mutableListOf<String>()
        val characterQueries = mutableListOf<String>()
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
            feeds += "$feed#$page"
            return onFeed(feed, page)
        }
        override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage = error("plain search must not be used")
        override suspend fun searchAdvanced(kind: MediaKind, query: MediaSearchQuery, page: Int): FeedPage {
            advanced += query to page
            return onAdvanced(query, page)
        }
        override suspend fun searchCharacters(query: String, page: Int): List<CharacterRef> {
            characterQueries += "$query#$page"
            return onCharacters(query, page)
        }
        override suspend fun media(kind: MediaKind, ref: MediaRef): Media? = null
        override suspend fun episodes(ref: MediaRef): List<Episode> = emptyList()
    }

    @Test
    fun `typing is debounced and only the last query hits the repository`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(onAdvanced = { _, p -> FeedPage(listOf(media(1)), p, false) })
        val vm = SearchViewModel(repo, debounceMillis = 300)

        vm.onQueryChange("n")
        advanceTimeBy(100)
        vm.onQueryChange("naru")
        advanceTimeBy(100)
        vm.onQueryChange("naruto")
        advanceTimeBy(301)
        advanceUntilIdle()

        assertEquals(listOf("naruto"), repo.advanced.map { it.first.query })
        assertEquals(MediaSort.RELEVANCE, repo.advanced.single().first.sort)
        assertIs<SearchResults.Ok>(vm.state.value.results)
    }

    @Test
    fun `states go idle - loading - empty - error and clear returns to idle`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(onAdvanced = { q, p ->
            when (q.query) {
                "none" -> FeedPage(emptyList(), p, false)
                "boom" -> throw AppException(AppError.RateLimited("anilist"))
                else -> FeedPage(listOf(media(1)), p, false)
            }
        })
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
    fun `filters go to the server query and scrolling loads page 2 of the same query`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(onAdvanced = { _, p ->
            if (p == 1) FeedPage(listOf(media(1), media(2)), 1, true) else FeedPage(listOf(media(2), media(3)), 2, false)
        })
        val vm = SearchViewModel(repo, debounceMillis = 10)
        vm.onQueryChange("frieren")
        vm.applySheet(SearchFilters(genres = listOf("Fantasy"), formats = listOf("TV")), adult = false)
        advanceUntilIdle()
        vm.loadMore()
        advanceUntilIdle()

        val last = repo.advanced.last()
        assertEquals(2, last.second)
        assertEquals("frieren", last.first.query)
        assertEquals(listOf("Fantasy"), last.first.genres)
        assertEquals(listOf("TV"), last.first.formats)
        assertEquals(false, last.first.adult)
        val ok = assertIs<SearchResults.Ok>(vm.state.value.results)
        assertEquals(listOf(1, 2, 3), ok.items.map { it.ref.anilistId })
        assertEquals(false, ok.hasNext)
    }

    @Test
    fun `every filter of the sheet reaches MediaSearchQuery`() {
        val q = SearchFilters(
            genres = listOf("Action"), tags = listOf("Magic"), formats = listOf("MOVIE"), year = 2023, season = "FALL",
            status = "FINISHED", yearFrom = 2000, yearTo = 2010, episodesFrom = 1, episodesTo = 13, scoreFrom = 70, scoreTo = 90,
            sort = MediaSort.SCORE,
        ).toQuery(text = " x ", adult = true, studio = "Madhouse")
        assertEquals(
            MediaSearchQuery(
                query = "x", genres = listOf("Action"), tags = listOf("Magic"), formats = listOf("MOVIE"), status = "FINISHED",
                season = "FALL", seasonYear = 2023, yearFrom = 2000, yearTo = 2010, episodesFrom = 1, episodesTo = 13,
                scoreFrom = 70, scoreTo = 90, sort = MediaSort.SCORE, adult = true, studio = "Madhouse",
            ),
            q,
        )
        assertEquals(11, SearchFilters(genres = listOf("A", "B"), tags = listOf("T"), formats = listOf("TV"), year = 1, season = "F",
            status = "S", yearFrom = 1, episodesTo = 2, scoreFrom = 3, sort = MediaSort.TITLE).activeCount)
    }

    @Test
    fun `adult results are dropped unless the toggle is on`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(onAdvanced = { _, p -> FeedPage(listOf(media(1), media(2, listOf("Hentai"))), p, false) })
        val vm = SearchViewModel(repo, debounceMillis = 10)
        vm.onQueryChange("x")
        advanceUntilIdle()
        assertEquals(listOf(1), vm.state.value.shown.map { it.ref.anilistId })
        assertEquals(false, repo.advanced.last().first.adult)

        vm.setAdult(true)
        advanceUntilIdle()
        assertEquals(true, repo.advanced.last().first.adult)
        assertEquals(listOf(1, 2), vm.state.value.shown.map { it.ref.anilistId })
    }

    @Test
    fun `submit records recent searches newest first, deduped and capped, and clear empties them`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val store = InMemorySearchHistoryStore(listOf("b", "frieren"))
        val vm = SearchViewModel(FakeMetadata(onAdvanced = { _, p -> FeedPage(emptyList(), p, false) }), debounceMillis = 10, historyStore = store)

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

        vm.clearHistory()
        assertEquals(emptyList(), store.load())
        assertEquals(emptyList(), vm.state.value.history)
    }

    @Test
    fun `an initial feed browses that feed and pages it`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(
            onFeed = { _, p -> FeedPage(listOf(media(p * 10)), p, p == 1) },
            onAdvanced = { _, p -> FeedPage(emptyList(), p, false) },
        )
        val vm = SearchViewModel(repo, debounceMillis = 10)

        vm.applyInitial(query = null, feed = FeedKind.AIRING, genre = null)
        advanceUntilIdle()
        vm.loadMore()
        advanceUntilIdle()

        assertEquals(listOf("AIRING#1", "AIRING#2"), repo.feeds)
        assertEquals(emptyList(), repo.advanced)
        assertEquals(listOf(10, 20), assertIs<SearchResults.Ok>(vm.state.value.results).items.map { it.ref.anilistId })

        // Applying the same initial values again (coming back to the screen) does not reload.
        vm.applyInitial(query = null, feed = FeedKind.AIRING, genre = null)
        advanceUntilIdle()
        assertEquals(2, repo.feeds.size)
    }

    @Test
    fun `a feed narrowed by a filter becomes a server query with the feed's sort and status`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(onAdvanced = { _, p -> FeedPage(listOf(media(1)), p, false) })
        val vm = SearchViewModel(repo, debounceMillis = 10)
        vm.selectFeed(FeedKind.AIRING)
        vm.setFilters(SearchFilters(genres = listOf("Drama")))
        advanceUntilIdle()

        val q = repo.advanced.last().first
        assertEquals("RELEASING", q.status)
        assertEquals(listOf("Drama"), q.genres)
        assertNull(q.query)
    }

    @Test
    fun `an initial genre and an initial studio browse server-side`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(onAdvanced = { _, p -> FeedPage(listOf(media(1, listOf("Action"))), p, false) })
        val vm = SearchViewModel(repo, debounceMillis = 10)

        vm.applyInitial(query = null, feed = null, genre = "Action")
        advanceUntilIdle()
        assertEquals(listOf("Action"), repo.advanced.last().first.genres)
        assertEquals(MediaSort.POPULARITY, repo.advanced.last().first.sort)

        vm.applyInitial(query = null, feed = null, genre = null, studio = "Madhouse")
        advanceUntilIdle()
        assertEquals("Madhouse", repo.advanced.last().first.studio)
        assertEquals("Madhouse", vm.state.value.studio)

        vm.clearStudio()
        advanceUntilIdle()
        assertEquals(SearchResults.Idle, vm.state.value.results)
    }

    @Test
    fun `character type searches characters and pages by PER_PAGE_CHARACTERS`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val repo = FakeMetadata(
            onAdvanced = { _, p -> FeedPage(emptyList(), p, false) },
            onCharacters = { _, p -> (1..if (p == 1) 20 else 3).map { CharacterRef(p * 100 + it, "C$it") } },
        )
        val vm = SearchViewModel(repo, debounceMillis = 10)
        vm.setType(SearchType.CHARACTERS)
        vm.onQueryChange("frieren")
        advanceUntilIdle()
        assertTrue(assertIs<SearchResults.Ok>(vm.state.value.results).hasNext)
        vm.loadMore()
        advanceUntilIdle()

        assertEquals(listOf("frieren#1", "frieren#2"), repo.characterQueries)
        val ok = assertIs<SearchResults.Ok>(vm.state.value.results)
        assertEquals(23, ok.characters.size)
        assertEquals(false, ok.hasNext)
        assertEquals(emptyList(), repo.advanced)
    }

    @Test
    fun `trending with a window other than today goes to the trending source`() = runTest {
        Dispatchers.setMain(StandardTestDispatcher(testScheduler))
        val calls = mutableListOf<TrendWindow>()
        val repo = FakeMetadata(onFeed = { _, p -> FeedPage(listOf(media(1)), p, false) }, onAdvanced = { _, p -> FeedPage(emptyList(), p, false) })
        val vm = SearchViewModel(repo, debounceMillis = 10, trending = TrendingSource { w, p, _ ->
            calls += w
            FeedPage(listOf(media(7)), p, false)
        })
        vm.selectFeed(FeedKind.TRENDING)
        advanceUntilIdle()
        vm.setTrendWindow(TrendWindow.WEEK)
        advanceUntilIdle()

        assertEquals(listOf("TRENDING#1"), repo.feeds)
        assertEquals(listOf(TrendWindow.WEEK), calls)
        assertEquals(listOf(7), vm.state.value.shown.map { it.ref.anilistId })
    }

    @Test
    fun `feeds map to AniList queries`() {
        assertEquals(MediaSearchQuery(status = "NOT_YET_RELEASED", sort = MediaSort.POPULARITY), feedQuery(FeedKind.UPCOMING))
        assertEquals(
            MediaSearchQuery(season = "SPRING", seasonYear = 2026, sort = MediaSort.POPULARITY),
            feedQuery(FeedKind.SEASONAL, LocalDate.of(2026, 5, 2)),
        )
    }

    @Test
    fun `range sliders at their bounds mean no limit and tags hide spoilers`() {
        assertEquals(null to null, rangeToBounds(0, 100, 0, 100))
        assertEquals(70 to null, rangeToBounds(70, 100, 0, 100))
        assertEquals(null to 12, rangeToBounds(0, 12, 0, 200))
        val tags = listOf(MediaTag("Magic"), MediaTag("Twist", isSpoiler = true), MediaTag("Elf"))
        assertEquals(listOf("Magic", "Elf"), visibleTags(tags, emptyList(), "", spoilers = false).map { it.name })
        assertEquals(listOf("Twist", "Magic", "Elf"), visibleTags(tags, listOf("Twist"), "", spoilers = false).map { it.name })
        assertEquals(listOf("Elf"), visibleTags(tags, emptyList(), "el", spoilers = true).map { it.name })
    }

    @Test
    fun `image matches keep the best hit per title above the threshold`() {
        fun m(id: Int, sim: Double) = ImageMatch(MediaRef(anilistId = id), "T$id", 1, 0.0, 1.0, sim, null, null)
        val list = listOf(m(1, 0.80), m(1, 0.97), m(2, 0.90), m(3, 0.50))
        assertEquals(listOf(1, 2, 3), bestPerTitle(list).map { it.ref.anilistId })
        assertEquals(listOf(0.97, 0.90), rankMatches(list, 0.85f).map { it.similarity })
    }
}
