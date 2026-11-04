package app.tatakai.next.feature.search

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.ui.media.distinctByKey
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

sealed interface SearchResults {
    /** Nothing typed yet (and no feed / genre to browse): show the prompt and the recent searches. */
    data object Idle : SearchResults

    data object Loading : SearchResults

    /** The provider returned no items. */
    data object Empty : SearchResults

    data class Ok(val items: List<Media>, val page: Int, val hasNext: Boolean, val loadingMore: Boolean = false) : SearchResults

    data class Err(val cause: AppError) : SearchResults
}

/** Type chips (prototype): only anime exists in M0; manga and novel are shown disabled with "Coming in M1". */
enum class SearchType(val enabled: Boolean) { ANIME(true), MANGA(false), NOVEL(false) }

/**
 * Filters of the bottom sheet (prototype `filters`). Applied on the client to the loaded pages: the metadata API has no filter
 * parameters. [season] / [status] hold provider values (`WINTER`.., `RELEASING` / `FINISHED` / `NOT_YET_RELEASED`).
 */
data class SearchFilters(
    val genre: String? = null,
    val year: Int? = null,
    val season: String? = null,
    val status: String? = null,
) {
    val activeCount: Int get() = listOfNotNull(genre, year, season, status).size

    val isEmpty: Boolean get() = activeCount == 0

    fun matches(media: Media): Boolean =
        (genre == null || media.genres.any { it.equals(genre, ignoreCase = true) }) &&
            (year == null || media.year == year) &&
            (season == null || media.season.equals(season, ignoreCase = true)) &&
            (status == null || media.status.equals(status, ignoreCase = true))
}

data class SearchUiState(
    val query: String = "",
    val results: SearchResults = SearchResults.Idle,
    /** Recent searches, newest first (at most [SEARCH_HISTORY_SIZE]). */
    val history: List<String> = emptyList(),
    val filters: SearchFilters = SearchFilters(),
    /** Feed browsed while the query is empty (Home "Ver todo" -> Route.SearchFor(feed)). */
    val feed: FeedKind? = null,
    /** Enabled extensions: source chips (extension search arrives in M1, so they are shown disabled). */
    val extensions: List<InstalledExtension> = emptyList(),
) {
    /** What an empty query browses: the explicit feed, else Popular filtered by the genre (Route.SearchFor(genre)), else nothing. */
    val browseFeed: FeedKind? get() = browseFeedFor(query.trim(), feed, filters)

    /** The loaded results after the client-side [filters]. */
    val shown: List<Media> get() = (results as? SearchResults.Ok)?.items?.filter(filters::matches).orEmpty()
}

internal fun browseFeedFor(query: String, feed: FeedKind?, filters: SearchFilters): FeedKind? = when {
    query.isNotEmpty() -> null
    feed != null -> feed
    filters.genre != null -> FeedKind.POPULAR
    else -> null
}

@OptIn(FlowPreview::class)
class SearchViewModel(
    private val metadata: MetadataRepository,
    debounceMillis: Long = DEFAULT_DEBOUNCE_MS,
    private val historyStore: SearchHistoryStore = InMemorySearchHistoryStore(),
    extensions: ExtensionRepository? = null,
) : ViewModel() {

    /** One fetch: a text search, or a feed page when the query is empty. */
    private data class Request(val query: String, val feed: FeedKind?)

    private val _state = MutableStateFlow(SearchUiState(history = historyStore.load()))
    val state: StateFlow<SearchUiState> = _state.asStateFlow()

    private val queries = MutableStateFlow("")
    private var lastRequest: Request? = null
    private var loadMoreJob: Job? = null
    private var appliedInitial: Triple<String?, FeedKind?, String?>? = null

    init {
        viewModelScope.launch {
            queries
                .debounce(debounceMillis)
                .map { it.trim() }
                .distinctUntilChanged()
                .collectLatest { q -> runSearch(q) }
        }
        if (extensions != null) {
            viewModelScope.launch {
                extensions.installed
                    .catch { emit(emptyList()) }
                    .collect { list -> _state.update { it.copy(extensions = list.filter { e -> e.enabled }) } }
            }
        }
    }

    /**
     * Pre-fills the screen from Route.SearchFor (query / feed / genre). Applied once per distinct triple, so coming back to the
     * screen keeps what the user typed since.
     */
    fun applyInitial(query: String?, feed: FeedKind?, genre: String?) {
        val key = Triple(query, feed, genre)
        if (key == appliedInitial) return
        appliedInitial = key
        if (query == null && feed == null && genre == null) return
        _state.update { it.copy(query = query.orEmpty(), feed = feed, filters = SearchFilters(genre = genre)) }
        queries.value = query.orEmpty()
        refresh()
    }

    fun onQueryChange(value: String) {
        _state.update { it.copy(query = value) }
        queries.value = value
    }

    /** IME "search": runs now and records the query in the recent searches. */
    fun submit() {
        val q = _state.value.query
        recordHistory(q)
        refresh()
    }

    fun useHistory(query: String) {
        onQueryChange(query)
        submit()
    }

    fun removeHistory(query: String) {
        val next = _state.value.history.filter { it != query }
        historyStore.save(next)
        _state.update { it.copy(history = next) }
    }

    fun clear() = onQueryChange("")

    fun setFilters(filters: SearchFilters) {
        _state.update { it.copy(filters = filters) }
        // A genre can switch an empty query into browsing Popular; with a query the filters only narrow the loaded pages.
        if (_state.value.query.isBlank()) refresh()
    }

    fun resetFilters() = setFilters(SearchFilters())

    /** Drops the browsed feed chip (Route.SearchFor(feed)). */
    fun clearFeed() {
        _state.update { it.copy(feed = null) }
        if (_state.value.query.isBlank()) refresh()
    }

    /** Re-runs the current request immediately (Retry button). */
    fun retry() {
        lastRequest = null
        refresh()
    }

    fun loadMore() {
        val current = _state.value.results as? SearchResults.Ok ?: return
        val request = lastRequest ?: return
        if (!current.hasNext || current.loadingMore) return
        _state.update { it.copy(results = current.copy(loadingMore = true)) }
        loadMoreJob?.cancel()
        loadMoreJob = viewModelScope.launch {
            runCatchingApp { fetch(request, current.page + 1) }
                .onSuccess { page ->
                    if (lastRequest != request) return@onSuccess
                    _state.update {
                        it.copy(
                            results = SearchResults.Ok(
                                items = (current.items + page.items).distinctByKey(),
                                page = page.page,
                                hasNext = page.hasNext,
                            ),
                        )
                    }
                }
                .onFailure {
                    if (lastRequest == request) _state.update { s -> s.copy(results = current.copy(loadingMore = false)) }
                }
        }
    }

    /**
     * Runs the current request now. Never cancels an in-flight run: a duplicate request is skipped by [runSearch] and a superseded
     * one drops its result (it compares [lastRequest] after the fetch).
     */
    private fun refresh() {
        viewModelScope.launch { runSearch(_state.value.query.trim()) }
    }

    private fun recordHistory(query: String) {
        val next = _state.value.history.withSearch(query)
        if (next == _state.value.history) return
        historyStore.save(next)
        _state.update { it.copy(history = next) }
    }

    private suspend fun runSearch(q: String) {
        val s = _state.value
        val request = Request(q, browseFeedFor(q, s.feed, s.filters))
        val running = s.results is SearchResults.Loading || s.results is SearchResults.Ok || s.results is SearchResults.Empty
        if (request == lastRequest && running) return
        loadMoreJob?.cancel()
        lastRequest = request
        if (request.query.isEmpty() && request.feed == null) {
            _state.update { it.copy(results = SearchResults.Idle) }
            return
        }
        _state.update { it.copy(results = SearchResults.Loading) }
        val result = runCatchingApp { fetch(request, 1) }
        if (lastRequest != request) return
        _state.update {
            it.copy(
                results = result.fold(
                    onSuccess = { page ->
                        val items = page.items.distinctByKey()
                        if (items.isEmpty()) SearchResults.Empty else SearchResults.Ok(items, page.page, page.hasNext)
                    },
                    onFailure = { e -> SearchResults.Err(e.toAppError()) },
                ),
            )
        }
    }

    private suspend fun fetch(request: Request, page: Int): FeedPage {
        val feed = request.feed
        return if (request.query.isEmpty() && feed != null) {
            metadata.feed(MediaKind.ANIME, feed, page)
        } else {
            metadata.search(MediaKind.ANIME, request.query, page)
        }
    }

    companion object {
        const val DEFAULT_DEBOUNCE_MS = 350L
    }
}
