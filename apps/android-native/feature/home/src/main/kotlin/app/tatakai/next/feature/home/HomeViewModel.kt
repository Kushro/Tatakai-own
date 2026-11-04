package app.tatakai.next.feature.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.SettingsRepository
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
            feeds.all { val s = it.state; s is LoadState.Ok && s.value.isEmpty() }
}

val HERO_FEED = FeedKind.TRENDING
const val HERO_SIZE = 5
const val GENRE_CHIPS = 10
val HOME_FEEDS = listOf(FeedKind.TRENDING, FeedKind.AIRING, FeedKind.SEASONAL, FeedKind.POPULAR, FeedKind.TOP_RATED)

class HomeViewModel(
    private val metadata: MetadataRepository,
    history: HistoryRepository,
    settings: SettingsRepository? = null,
    private val extensions: ExtensionRepository? = null,
) : ViewModel() {

    private val feeds = MutableStateFlow(HOME_FEEDS.map { FeedRow(it, LoadState.Loading) })
    private val extensionRows = MutableStateFlow<LoadState<List<ExtensionHomeRow>>?>(null)

    private val providers = settings?.settings?.map { it.providerPriority }?.distinctUntilChanged()
        ?: flowOf(AppSettings.DEFAULT_PROVIDER_PRIORITY)

    val state: StateFlow<HomeUiState> = combine(
        feeds,
        history.continueWatching(CONTINUE_LIMIT).catch { emit(emptyList()) },
        providers.catch { emit(AppSettings.DEFAULT_PROVIDER_PRIORITY) },
        extensionRows,
    ) { rows, continueWatching, providerIds, extRows ->
        HomeUiState(feeds = rows, continueWatching = continueWatching, providers = providerIds, extensionRows = extRows)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), HomeUiState())

    init {
        HOME_FEEDS.forEach(::load)
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
            val result = runCatchingApp { repo.customHomeRows().filter { it.items.isNotEmpty() } }
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
    }
}
