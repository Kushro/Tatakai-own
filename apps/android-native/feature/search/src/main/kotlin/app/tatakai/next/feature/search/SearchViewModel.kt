package app.tatakai.next.feature.search

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.metadata.TrendWindow
import app.tatakai.next.model.AppError
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.MediaTag
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PER_PAGE_CHARACTERS
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.TitleLanguage
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
import java.time.LocalDate

sealed interface SearchResults {
    /** Nothing typed yet (and no feed / genre / studio to browse): show the prompt, the feeds and the recent searches. */
    data object Idle : SearchResults

    data object Loading : SearchResults

    /** The provider returned no items. */
    data object Empty : SearchResults

    /**
     * One kind of hit per search type: [items] (anime), [characters] (type Personajes) or [extensionItems] (an extension chip).
     * [hasNext] comes from the provider (`Page.pageInfo.hasNextPage`).
     */
    data class Ok(
        val items: List<Media>,
        val page: Int,
        val hasNext: Boolean,
        val loadingMore: Boolean = false,
        val characters: List<CharacterRef> = emptyList(),
        val extensionItems: List<ExtensionHomeItem> = emptyList(),
    ) : SearchResults {
        val size: Int get() = items.size + characters.size + extensionItems.size
    }

    data class Err(val cause: AppError) : SearchResults
}

/**
 * Search types (classic unified search). Manga and novels are the reader of wave B and are not offered here yet; [CHARACTERS] is the
 * AniList character search (`MetadataRepository.searchCharacters`).
 */
enum class SearchType { ANIME, CHARACTERS }

/**
 * Filters of the bottom sheet (classic `SearchPage` anime filters), applied SERVER-SIDE through [MediaSearchQuery]. [season] / [status]
 * / [formats] hold AniList values (`WINTER`, `RELEASING`, `TV`...); [year] is the exact season year; the ranges are inclusive; [sort] null
 * = automatic (relevance with text, popularity when browsing).
 */
data class SearchFilters(
    val genres: List<String> = emptyList(),
    val tags: List<String> = emptyList(),
    val formats: List<String> = emptyList(),
    val year: Int? = null,
    val season: String? = null,
    val status: String? = null,
    val yearFrom: Int? = null,
    val yearTo: Int? = null,
    val episodesFrom: Int? = null,
    val episodesTo: Int? = null,
    val scoreFrom: Int? = null,
    val scoreTo: Int? = null,
    val sort: MediaSort? = null,
) {
    val activeCount: Int
        get() = genres.size + tags.size + formats.size +
            listOfNotNull(year, season, status, sort).size +
            (if (yearFrom != null || yearTo != null) 1 else 0) +
            (if (episodesFrom != null || episodesTo != null) 1 else 0) +
            (if (scoreFrom != null || scoreTo != null) 1 else 0)

    val isEmpty: Boolean get() = activeCount == 0

    /** Single-genre compatibility (Route.SearchFor(genre)). */
    val genre: String? get() = genres.firstOrNull()

    /** The query AniList receives: these filters + [text] / [studio] / [adult], on top of [base] (a feed mapped to sort / status). */
    fun toQuery(text: String?, adult: Boolean, studio: String? = null, base: MediaSearchQuery = MediaSearchQuery()): MediaSearchQuery {
        val hasText = !text.isNullOrBlank()
        return base.copy(
            query = text?.trim()?.takeIf { it.isNotEmpty() },
            genres = genres,
            tags = tags,
            formats = formats,
            status = status ?: base.status,
            season = season ?: base.season,
            seasonYear = year ?: base.seasonYear,
            yearFrom = yearFrom,
            yearTo = yearTo,
            episodesFrom = episodesFrom,
            episodesTo = episodesTo,
            scoreFrom = scoreFrom,
            scoreTo = scoreTo,
            sort = sort ?: if (hasText) MediaSort.RELEVANCE else base.sort,
            adult = adult,
            studio = studio,
        )
    }
}

/** A feed of Buscar (Home "Ver todo", the Explorar chips) as an AniList query, so the filters can narrow it server-side. */
internal fun feedQuery(feed: FeedKind, today: LocalDate = LocalDate.now()): MediaSearchQuery = when (feed) {
    FeedKind.TRENDING -> MediaSearchQuery(sort = MediaSort.TRENDING)
    FeedKind.POPULAR -> MediaSearchQuery(sort = MediaSort.POPULARITY)
    FeedKind.TOP_RATED -> MediaSearchQuery(sort = MediaSort.SCORE)
    FeedKind.AIRING -> MediaSearchQuery(status = "RELEASING", sort = MediaSort.POPULARITY)
    FeedKind.UPCOMING -> MediaSearchQuery(status = "NOT_YET_RELEASED", sort = MediaSort.POPULARITY)
    FeedKind.RECENTLY_COMPLETED -> MediaSearchQuery(status = "FINISHED", sort = MediaSort.START_DATE)
    FeedKind.SEASONAL -> MediaSearchQuery(
        season = when (today.monthValue) { in 1..3 -> "WINTER"; in 4..6 -> "SPRING"; in 7..9 -> "SUMMER"; else -> "FALL" },
        seasonYear = today.year,
        sort = MediaSort.POPULARITY,
    )
}

/** Extension chip of the source row: enabled only when the extension has a searchable catalogue (`custom` sources). */
data class ExtensionChip(val extension: InstalledExtension, val searchable: Boolean)

data class SearchUiState(
    val query: String = "",
    val results: SearchResults = SearchResults.Idle,
    /** Recent searches, newest first (at most [SEARCH_HISTORY_SIZE]). */
    val history: List<String> = emptyList(),
    val filters: SearchFilters = SearchFilters(),
    /** Feed browsed while the query is empty (Home "Ver todo" -> Route.SearchFor(feed), Explorar chips). */
    val feed: FeedKind? = null,
    /** Time window of the Tendencias feed (classic TrendingPage selector). */
    val trendWindow: TrendWindow = TrendWindow.TODAY,
    /** Studio catalogue (Details studio chip -> Route.SearchFor(studio)). */
    val studio: String? = null,
    val type: SearchType = SearchType.ANIME,
    /** Selected extension chip (search inside that extension); null = metadata. */
    val extensionId: String? = null,
    val extensions: List<ExtensionChip> = emptyList(),
    /** Include 18+ titles (initial value AppSettings.showAdult, then the sheet's toggle). */
    val adult: Boolean = false,
    val titleLanguage: TitleLanguage = TitleLanguage.ROMAJI,
    /** Provider catalogues for the sheet (fallback: [KNOWN_GENRES]); tags are loaded on the first sheet open. */
    val genreOptions: List<String> = KNOWN_GENRES,
    val tagOptions: List<MediaTag> = emptyList(),
    val tagsLoading: Boolean = false,
) {
    /** The loaded anime results; with 18+ off, anything the provider still tags Hentai is dropped (the query already excludes isAdult). */
    val shown: List<Media>
        get() = (results as? SearchResults.Ok)?.items?.filter { adult || !it.isProbablyAdult() }.orEmpty()

    /** Something is browsed with an empty query (feed, filters, studio). */
    val browsing: Boolean get() = query.isBlank() && (feed != null || studio != null || !filters.isEmpty)
}

/** AniList's adult titles carry the Hentai genre ([Media] has no isAdult flag); used to mark / drop them. */
fun Media.isProbablyAdult(): Boolean = genres.any { it.equals("Hentai", ignoreCase = true) }

/** What one fetch asks for; two equal requests are the same search. */
internal data class SearchRequest(
    val query: String,
    val type: SearchType,
    val extensionId: String?,
    val feed: FeedKind?,
    val window: TrendWindow,
    val studio: String?,
    val filters: SearchFilters,
    val adult: Boolean,
) {
    val idle: Boolean
        get() = when {
            type == SearchType.CHARACTERS || extensionId != null -> query.isBlank()
            else -> query.isBlank() && feed == null && studio == null && filters.isEmpty
        }
}

/** Tendencias with a time window other than "today" (:core:metadata `TrendingFeed`, bound in searchModule). */
fun interface TrendingSource {
    suspend fun page(window: TrendWindow, page: Int, adult: Boolean): FeedPage
}

@OptIn(FlowPreview::class)
class SearchViewModel(
    private val metadata: MetadataRepository,
    debounceMillis: Long = DEFAULT_DEBOUNCE_MS,
    private val historyStore: SearchHistoryStore = InMemorySearchHistoryStore(),
    private val extensions: ExtensionRepository? = null,
    settings: SettingsRepository? = null,
    private val trending: TrendingSource? = null,
) : ViewModel() {

    private val _state = MutableStateFlow(
        SearchUiState(
            history = historyStore.load(),
            adult = settings?.settings?.value?.showAdult ?: false,
            titleLanguage = settings?.settings?.value?.titleLanguage ?: TitleLanguage.ROMAJI,
        ),
    )
    val state: StateFlow<SearchUiState> = _state.asStateFlow()

    private val queries = MutableStateFlow("")
    private var lastRequest: SearchRequest? = null
    private var loadMoreJob: Job? = null
    private var appliedInitial: List<Any?>? = null
    private var tagsRequested = false
    private var lastShowAdult: Boolean? = settings?.settings?.value?.showAdult

    init {
        viewModelScope.launch {
            queries
                .debounce(debounceMillis)
                .map { it.trim() }
                .distinctUntilChanged()
                .collectLatest { runSearch() }
        }
        if (extensions != null) {
            viewModelScope.launch {
                extensions.installed
                    .catch { emit(emptyList()) }
                    .collect { list ->
                        val chips = list.filter { it.enabled }.map { ExtensionChip(it, searchable = it.type == CUSTOM_TYPE) }
                        _state.update { s ->
                            val keep = s.extensionId?.takeIf { id -> chips.any { it.extension.id == id && it.searchable } }
                            s.copy(extensions = chips, extensionId = keep)
                        }
                    }
            }
        }
        if (settings != null) {
            viewModelScope.launch {
                settings.settings.collect { s ->
                    // The Settings toggle is the initial value; a change there wins over the sheet's last choice.
                    val adultChanged = lastShowAdult != null && lastShowAdult != s.showAdult
                    lastShowAdult = s.showAdult
                    _state.update { it.copy(titleLanguage = s.titleLanguage, adult = if (adultChanged) s.showAdult else it.adult) }
                    if (adultChanged) refresh()
                }
            }
        }
        viewModelScope.launch {
            runCatchingApp { metadata.genres(MediaKind.ANIME) }.getOrNull()
                ?.takeIf { it.isNotEmpty() }
                ?.let { genres -> _state.update { it.copy(genreOptions = genres) } }
        }
    }

    /**
     * Pre-fills the screen from Route.SearchFor (query / feed / genre / studio). Applied once per distinct value set, so coming back to
     * the screen keeps what the user typed since.
     */
    fun applyInitial(query: String?, feed: FeedKind?, genre: String?, studio: String? = null) {
        val key = listOf(query, feed, genre, studio)
        if (key == appliedInitial) return
        appliedInitial = key
        if (query == null && feed == null && genre == null && studio == null) return
        _state.update {
            it.copy(
                query = query.orEmpty(),
                feed = feed,
                filters = SearchFilters(genres = listOfNotNull(genre)),
                studio = studio,
                type = SearchType.ANIME,
                extensionId = null,
            )
        }
        queries.value = query.orEmpty()
        refresh()
    }

    fun onQueryChange(value: String) {
        _state.update { it.copy(query = value) }
        queries.value = value
    }

    /** IME "search": runs now and records the query in the recent searches. */
    fun submit() {
        recordHistory(_state.value.query)
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

    /** Settings › Metadatos › "Borrar búsquedas recientes" (also offered under the list). */
    fun clearHistory() {
        historyStore.save(emptyList())
        _state.update { it.copy(history = emptyList()) }
    }

    /** The store may have been cleared from Settings while this screen was in the back stack. */
    fun reloadHistory() {
        val stored = historyStore.load()
        if (stored != _state.value.history) _state.update { it.copy(history = stored) }
    }

    fun clear() = onQueryChange("")

    fun setFilters(filters: SearchFilters) {
        _state.update { it.copy(filters = filters) }
        refresh()
    }

    /** "Limpiar": every filter off, the search runs again. */
    fun resetFilters() = setFilters(SearchFilters())

    fun setAdult(adult: Boolean) {
        if (adult == _state.value.adult) return
        _state.update { it.copy(adult = adult) }
        refresh()
    }

    /** Applies the sheet's draft (filters + 18+) as one search. */
    fun applySheet(filters: SearchFilters, adult: Boolean) {
        _state.update { it.copy(filters = filters, adult = adult) }
        refresh()
    }

    /** Explorar chip / Home "Ver todo": browse [feed] (query cleared, studio dropped). */
    fun selectFeed(feed: FeedKind) {
        _state.update { it.copy(feed = feed, query = "", studio = null, type = SearchType.ANIME, extensionId = null) }
        queries.value = ""
        refresh()
    }

    /** Drops the browsed feed chip. */
    fun clearFeed() {
        _state.update { it.copy(feed = null) }
        refresh()
    }

    fun setTrendWindow(window: TrendWindow) {
        if (window == _state.value.trendWindow) return
        _state.update { it.copy(trendWindow = window) }
        refresh()
    }

    fun clearStudio() {
        _state.update { it.copy(studio = null) }
        refresh()
    }

    fun setType(type: SearchType) {
        if (type == _state.value.type) return
        _state.update { it.copy(type = type, extensionId = if (type == SearchType.ANIME) it.extensionId else null) }
        refresh()
    }

    /** Extension chip (null = back to the metadata source). Only searchable chips can be selected. */
    fun selectExtension(id: String?) {
        val chip = id?.let { i -> _state.value.extensions.firstOrNull { it.extension.id == i } }
        if (id != null && chip?.searchable != true) return
        _state.update { it.copy(extensionId = id, type = SearchType.ANIME) }
        refresh()
    }

    /** An extension result: look its title up in the metadata provider (back to the metadata source). */
    fun openExtensionItem(item: ExtensionHomeItem) {
        _state.update { it.copy(extensionId = null, type = SearchType.ANIME, feed = null, studio = null) }
        onQueryChange(item.title)
        submit()
    }

    /** First open of the filters sheet: the AniList tag catalogue. */
    fun ensureTags() {
        if (tagsRequested) return
        tagsRequested = true
        _state.update { it.copy(tagsLoading = true) }
        viewModelScope.launch {
            val tags = runCatchingApp { metadata.tags(MediaKind.ANIME) }.getOrNull().orEmpty()
            if (tags.isEmpty()) tagsRequested = false
            _state.update { it.copy(tagOptions = tags, tagsLoading = false) }
        }
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
                .onSuccess { next ->
                    if (lastRequest != request) return@onSuccess
                    _state.update {
                        it.copy(
                            results = SearchResults.Ok(
                                items = (current.items + next.items).distinctByKey(),
                                page = next.page,
                                hasNext = next.hasNext,
                                characters = (current.characters + next.characters).distinctBy { c -> c.id },
                                extensionItems = current.extensionItems,
                            ),
                        )
                    }
                }
                .onFailure {
                    if (lastRequest == request) _state.update { s -> s.copy(results = current.copy(loadingMore = false)) }
                }
        }
    }

    private fun currentRequest(): SearchRequest {
        val s = _state.value
        return SearchRequest(
            query = s.query.trim(),
            type = s.type,
            extensionId = s.extensionId,
            feed = s.feed?.takeIf { s.query.isBlank() },
            window = s.trendWindow,
            studio = s.studio,
            filters = s.filters,
            adult = s.adult,
        )
    }

    /**
     * Runs the current request now. Never cancels an in-flight run: a duplicate request is skipped by [runSearch] and a superseded
     * one drops its result (it compares [lastRequest] after the fetch).
     */
    private fun refresh() {
        viewModelScope.launch { runSearch() }
    }

    private fun recordHistory(query: String) {
        val next = _state.value.history.withSearch(query)
        if (next == _state.value.history) return
        historyStore.save(next)
        _state.update { it.copy(history = next) }
    }

    private suspend fun runSearch() {
        val s = _state.value
        val request = currentRequest()
        val running = s.results is SearchResults.Loading || s.results is SearchResults.Ok || s.results is SearchResults.Empty
        if (request == lastRequest && running) return
        loadMoreJob?.cancel()
        lastRequest = request
        if (request.idle) {
            _state.update { it.copy(results = SearchResults.Idle) }
            return
        }
        _state.update { it.copy(results = SearchResults.Loading) }
        val result = runCatchingApp { fetch(request, 1) }
        if (lastRequest != request) return
        _state.update {
            it.copy(
                results = result.fold(
                    onSuccess = { ok -> if (ok.size == 0) SearchResults.Empty else ok },
                    onFailure = { e -> SearchResults.Err(e.toAppError()) },
                ),
            )
        }
    }

    private suspend fun fetch(request: SearchRequest, page: Int): SearchResults.Ok {
        if (request.type == SearchType.CHARACTERS) {
            val list = metadata.searchCharacters(request.query, page)
            return SearchResults.Ok(emptyList(), page, hasNext = list.size >= PER_PAGE_CHARACTERS, characters = list)
        }
        val extId = request.extensionId
        if (extId != null) {
            val items = extensions?.searchExtension(extId, request.query).orEmpty()
            return SearchResults.Ok(emptyList(), 1, hasNext = false, extensionItems = items)
        }
        val feed = request.feed
        val p: FeedPage = when {
            request.studio != null ->
                metadata.searchAdvanced(MediaKind.ANIME, request.filters.toQuery(request.query, request.adult, request.studio), page)
            request.query.isNotEmpty() ->
                metadata.searchAdvanced(MediaKind.ANIME, request.filters.toQuery(request.query, request.adult), page)
            feed == FeedKind.TRENDING && request.window != TrendWindow.TODAY && request.filters.isEmpty && trending != null ->
                trending.page(request.window, page, request.adult)
            feed != null && request.filters.isEmpty && !request.adult ->
                metadata.feed(MediaKind.ANIME, feed, page)
            feed != null ->
                metadata.searchAdvanced(MediaKind.ANIME, request.filters.toQuery(null, request.adult, base = feedQuery(feed)), page)
            else ->
                metadata.searchAdvanced(MediaKind.ANIME, request.filters.toQuery(null, request.adult), page)
        }
        val items = p.items.distinctByKey().filter { request.adult || !it.isProbablyAdult() }
        return SearchResults.Ok(items, p.page, p.hasNext)
    }

    companion object {
        const val DEFAULT_DEBOUNCE_MS = 350L
        private const val CUSTOM_TYPE = "custom"
    }
}

/** Provider genre vocabulary fallback (AniList genre names, shown untranslated like Media.genres). */
internal val KNOWN_GENRES = listOf(
    "Action", "Adventure", "Comedy", "Drama", "Ecchi", "Fantasy", "Horror", "Mahou Shoujo", "Mecha", "Music", "Mystery",
    "Psychological", "Romance", "Sci-Fi", "Slice of Life", "Sports", "Supernatural", "Thriller",
)
