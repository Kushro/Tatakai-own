package app.tatakai.next.feature.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AiringRepository
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.RecsRepository
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.torrent.TorrentRepository
import kotlinx.coroutines.Job
import app.tatakai.next.ui.media.distinctByKey
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** One independently loading row of Home. A failing row never breaks the others (02 §3). */
data class FeedRow(val kind: FeedKind, val state: LoadState<List<Media>>)

data class HomeUiState(
    val feeds: List<FeedRow> = HOME_FEEDS.map { FeedRow(it, LoadState.Loading) },
    val continueWatching: List<HistoryEntry> = emptyList(),
    /** Metadata provider ids in priority order (provider chips of the greeting; the first one is the active source). */
    val providers: List<String> = AppSettings.DEFAULT_PROVIDER_PRIORITY,
    /** Lazy `custom/home` rows of extensions; null = not requested yet (the row has not been scrolled into view). */
    val extensionRows: LoadState<List<ExtensionHomeRow>>? = null,
    /** parity wave B (worker r): the rows of 16 §3.1 (latest episodes, top 10, upcoming, favourites, for you, ...). */
    val discover: HomeDiscoverState = HomeDiscoverState(),
) {
    /** Hero carousel = first 5 trending items. */
    val hero: LoadState<List<Media>>
        get() = when (val s = feeds.firstOrNull { it.kind == HERO_FEED }?.state ?: LoadState.Loading) {
            is LoadState.Ok -> LoadState.Ok(s.value.take(HERO_SIZE))
            else -> s
        }

    /** Rows rendered below the hero (everything but the hero feed). */
    val rows: List<FeedRow> get() = feeds.filter { it.kind != HERO_FEED }

    /** Genre chips (most frequent genres of the loaded feeds, at most [GENRE_CHIPS]). */
    val genres: List<String>
        get() = feeds.asSequence()
            .mapNotNull { (it.state as? LoadState.Ok)?.value }
            .flatten()
            .flatMap { it.genres.asSequence() }
            .groupingBy { it }
            .eachCount()
            .entries
            .sortedWith(compareByDescending<Map.Entry<String, Int>> { it.value }.thenBy { it.key })
            .take(GENRE_CHIPS)
            .map { it.key }

    /** Every feed answered with nothing and there is nothing to continue: the Home empty state. */
    val isEmpty: Boolean
        get() = continueWatching.isEmpty() &&
            feeds.all { val s = it.state; s is LoadState.Ok && s.value.isEmpty() } &&
            !discover.hasContent
}

val HERO_FEED = FeedKind.TRENDING
const val HERO_SIZE = 5
const val GENRE_CHIPS = 10
val HOME_FEEDS = listOf(FeedKind.TRENDING, FeedKind.AIRING, FeedKind.SEASONAL, FeedKind.POPULAR, FeedKind.TOP_RATED)

/** Trending windows of :core:metadata `TrendingFeed` (bound in homeModule when metadataModule is loaded). */
fun interface HomeTrendSource {
    suspend fun page(window: TopWindow, adult: Boolean): List<Media>
}

class HomeViewModel(
    private val metadata: MetadataRepository,
    history: HistoryRepository,
    private val settings: SettingsRepository? = null,
    private val extensions: ExtensionRepository? = null,
    // --- parity wave B (16 §3.1, worker r), all optional so the wave android tests keep their fakes ---
    private val airing: AiringRepository? = null,
    private val recs: RecsRepository? = null,
    private val trending: HomeTrendSource? = null,
    downloads: DownloadRepository? = null,
    torrents: TorrentRepository? = null,
    private val nowSec: () -> Long = { System.currentTimeMillis() / 1000 },
) : ViewModel() {

    private val feeds = MutableStateFlow(HOME_FEEDS.map { FeedRow(it, LoadState.Loading) })
    private val extensionRows = MutableStateFlow<LoadState<List<ExtensionHomeRow>>?>(null)
    private val discover = MutableStateFlow(HomeDiscoverState())
    private var becauseJob: Job? = null
    private var genreJob: Job? = null

    private val providers = settings?.settings?.map { it.providerPriority }?.distinctUntilChanged()
        ?: flowOf(AppSettings.DEFAULT_PROVIDER_PRIORITY)

    val state: StateFlow<HomeUiState> = combine(
        feeds,
        history.continueWatching(CONTINUE_LIMIT).catch { emit(emptyList()) },
        providers.catch { emit(AppSettings.DEFAULT_PROVIDER_PRIORITY) },
        extensionRows,
        discover,
    ) { rows, continueWatching, providerIds, extRows, extra ->
        HomeUiState(feeds = rows, continueWatching = continueWatching, providers = providerIds, extensionRows = extRows, discover = extra)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), HomeUiState())

    private val adult: Boolean get() = settings?.settings?.value?.showAdult ?: false

    init {
        HOME_FEEDS.forEach(::load)
        loadLatest()
        loadTop10(TopWindow.TODAY)
        loadUpcoming()
        loadFavourites()
        loadGenreCloud()
        loadFollowed()
        loadForYou()
        viewModelScope.launch {
            settings?.settings?.map { it.matureContent }?.distinctUntilChanged()?.catch { emit(MatureContentMode.WARN) }?.collect { mode ->
                discover.update { it.copy(matureContent = mode) }
            }
        }
        viewModelScope.launch {
            history.continueWatching(CONTINUE_LIMIT).catch { emit(emptyList()) }
                .map { list -> list.firstOrNull { it.anilistId != null } }
                .distinctUntilChanged { a, b -> a?.anilistId == b?.anilistId }
                .collect { entry -> loadBecause(entry) }
        }
        if (downloads != null || torrents != null) {
            viewModelScope.launch {
                combine(
                    torrents?.status?.map { it.activeDownloads } ?: flowOf(0),
                    downloads?.items ?: flowOf(emptyList()),
                    torrents?.library ?: flowOf(emptyList()),
                ) { active, web, lib -> HomeLogic.activity(active, web, lib) }
                    .catch { }
                    .collect { a -> discover.update { it.copy(activity = a) } }
            }
        }
    }

    // ---------------------------------------------------------------- parity wave B rows (16 §3.1)

    private fun <T> launchRow(set: (HomeDiscoverState, LoadState<T>) -> HomeDiscoverState, block: suspend () -> T) {
        discover.update { set(it, LoadState.Loading) }
        viewModelScope.launch {
            val r = runCatchingApp { block() }
            discover.update { set(it, r.fold(onSuccess = { v -> LoadState.Ok(v) }, onFailure = { e -> LoadState.Err(e.toAppError()) })) }
        }
    }

    /** "Últimos episodios": AniList airing schedule of the last 7 days (checklist home.latest-episodes). */
    fun loadLatest() = launchRow<List<app.tatakai.next.model.AiringEpisode>>({ s, v -> s.copy(latest = v) }) {
        val now = nowSec()
        HomeLogic.latestEpisodes(metadata.airingSchedule(now - 7 * 86_400, now + 1), now)
    }

    /** Top 10 with Hoy / Semana / Mes (checklist home.top10). */
    fun loadTop10(window: TopWindow) {
        discover.update { it.copy(topWindow = window) }
        launchRow<List<Media>>({ s, v -> if (s.topWindow == window) s.copy(top10 = v) else s }) {
            val list = trending?.page(window, adult)
                ?: metadata.searchAdvanced(MediaKind.ANIME, MediaSearchQuery(sort = MediaSort.TRENDING, adult = adult)).items
            list.distinctByKey().take(TOP_SIZE)
        }
    }

    /** "Próximos estrenos" (status NOT_YET_RELEASED, checklist home.upcoming). */
    fun loadUpcoming() = launchRow<List<Media>>({ s, v -> s.copy(upcoming = v) }) {
        metadata.searchAdvanced(MediaKind.ANIME, MediaSearchQuery(status = "NOT_YET_RELEASED", sort = MediaSort.POPULARITY, adult = adult)).items.distinctByKey()
    }

    /** "Más favoritos" (AniList FAVOURITES_DESC, checklist home.most-favorite). */
    fun loadFavourites() = launchRow<List<Media>>({ s, v -> s.copy(favourites = v) }) {
        metadata.searchAdvanced(MediaKind.ANIME, MediaSearchQuery(sort = MediaSort.FAVOURITES, adult = adult)).items.distinctByKey()
    }

    /** "Novedades de lo que sigues" (checklist home.followed-updates). */
    fun loadFollowed() {
        val repo = airing ?: run {
            discover.update { it.copy(followed = LoadState.Ok(emptyList())) }
            return
        }
        launchRow<List<app.tatakai.next.model.AiringEpisode>>({ s, v -> s.copy(followed = v) }) { repo.newSinceLastVisit() }
    }

    /** "Para ti" preview (checklist home.recs.for-you). */
    fun loadForYou() {
        val repo = recs ?: run {
            discover.update { it.copy(forYou = LoadState.Ok(emptyList())) }
            return
        }
        launchRow<List<app.tatakai.next.model.RecommendationItem>>({ s, v -> s.copy(forYou = v) }) { repo.forYou(FOR_YOU_PREVIEW) }
    }

    private fun loadBecause(entry: HistoryEntry?) {
        becauseJob?.cancel()
        val id = entry?.anilistId
        if (entry == null || id == null) {
            discover.update { it.copy(because = null) }
            return
        }
        discover.update { it.copy(because = BecauseRow(entry.title, LoadState.Loading)) }
        becauseJob = viewModelScope.launch {
            val r = runCatchingApp { metadata.recommendations(MediaRef(anilistId = id, malId = entry.malId)).map { it.media }.distinctByKey() }
            discover.update {
                it.copy(because = BecauseRow(entry.title, r.fold(onSuccess = { v -> LoadState.Ok(v) }, onFailure = { e -> LoadState.Err(e.toAppError()) })))
            }
        }
    }

    fun retryBecause() {
        val title = discover.value.because?.sourceTitle ?: return
        viewModelScope.launch {
            val entry = state.value.continueWatching.firstOrNull { it.anilistId != null && it.title == title }
            loadBecause(entry)
        }
    }

    /** Genre catalogue of the provider (falls back to the genres of the loaded rows in the screen). */
    fun loadGenreCloud() {
        viewModelScope.launch {
            val genres = runCatchingApp { metadata.genres(MediaKind.ANIME) }.getOrNull().orEmpty()
                .filterNot { it.equals("Hentai", ignoreCase = true) && !adult }
            discover.update { it.copy(genreCloud = genres, genreRowsDone = genres.isEmpty()) }
        }
    }

    /** Called by the last item of Home: appends the next genre row (checklist home.infinite-genre-rows). */
    fun loadNextGenreRow(fallbackGenres: List<String>) {
        if (genreJob?.isActive == true) return
        val current = discover.value
        val catalogue = current.genreCloud.ifEmpty { fallbackGenres }
        val next = catalogue.firstOrNull { g -> current.genreRows.none { it.genre == g } }
        if (next == null) {
            discover.update { it.copy(genreRowsDone = true) }
            return
        }
        discover.update { it.copy(genreRows = it.genreRows + GenreRow(next, LoadState.Loading), genreRowsDone = false) }
        genreJob = viewModelScope.launch { fillGenre(next) }
    }

    fun retryGenre(genre: String) {
        discover.update { s -> s.copy(genreRows = s.genreRows.map { if (it.genre == genre) it.copy(state = LoadState.Loading) else it }) }
        viewModelScope.launch { fillGenre(genre) }
    }

    private suspend fun fillGenre(genre: String) {
        val r = runCatchingApp {
            metadata.searchAdvanced(MediaKind.ANIME, MediaSearchQuery(genres = listOf(genre), sort = MediaSort.POPULARITY, adult = adult)).items.distinctByKey()
        }
        val st: LoadState<List<Media>> = r.fold(onSuccess = { LoadState.Ok(it) }, onFailure = { LoadState.Err(it.toAppError()) })
        discover.update { s -> s.copy(genreRows = s.genreRows.map { if (it.genre == genre) it.copy(state = st) else it }) }
    }

    /** The user opened the calendar from the "Novedades" header: what they saw is no longer new. */
    fun markFollowedSeen() {
        val repo = airing ?: return
        viewModelScope.launch { runCatchingApp { repo.markVisited() } }
    }

    /** Reloads a single row (the "Retry" button of its inline error). */
    fun retry(kind: FeedKind) {
        setRow(kind, LoadState.Loading)
        load(kind)
    }

    /**
     * Called when the extension row scrolls into view (lazy, like the prototype's IntersectionObserver). Requests
     * `customHomeRows()` once; later calls are no-ops unless the previous attempt failed.
     */
    fun loadExtensionRows() {
        val current = extensionRows.value
        if (current != null && current !is LoadState.Err) return
        val repo = extensions
        if (repo == null) {
            extensionRows.value = LoadState.Ok(emptyList())
            return
        }
        extensionRows.value = LoadState.Loading
        viewModelScope.launch {
            val result = runCatchingApp {
                // parity wave B (16 §3.1 / §3.4): rows of the custom `watch` sources with the extension NAME; the wave android
                // `customHomeRows()` when no extension declares custom sources.
                val sources = runCatchingApp { repo.customSources("watch") }.getOrNull().orEmpty()
                val names = repo.installed.value.associate { it.id to it.name } + sources.associate { it.extensionId to it.extensionName }
                discover.update { it.copy(extensionNames = names) }
                val rows = if (sources.isNotEmpty()) {
                    sources.take(MAX_PROVIDER_SOURCES).flatMap { src ->
                        runCatchingApp { repo.providerHome(src.extensionId, src.sourceId) }.getOrNull().orEmpty()
                            .map { row -> if (row.title.isBlank()) row.copy(title = src.name) else row }
                    }
                } else {
                    repo.customHomeRows()
                }
                rows.filter { it.items.isNotEmpty() }
            }
            extensionRows.value = result.fold(
                onSuccess = { LoadState.Ok(it) },
                onFailure = { LoadState.Err(it.toAppError()) },
            )
        }
    }

    private fun load(kind: FeedKind) {
        viewModelScope.launch {
            val result = runCatchingApp { metadata.feed(MediaKind.ANIME, kind).items.distinctByKey() }
            setRow(
                kind,
                result.fold(
                    onSuccess = { LoadState.Ok(it) },
                    onFailure = { LoadState.Err(it.toAppError()) },
                ),
            )
        }
    }

    private fun setRow(kind: FeedKind, state: LoadState<List<Media>>) {
        feeds.update { rows -> rows.map { if (it.kind == kind) it.copy(state = state) else it } }
    }

    private companion object {
        const val CONTINUE_LIMIT = 20
        const val TOP_SIZE = 10
        const val FOR_YOU_PREVIEW = 10
        const val MAX_PROVIDER_SOURCES = 6
    }
}
