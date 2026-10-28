package app.tatakai.next.feature.home

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.ui.media.distinctByKey
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** One independently loading row of Home. A failing row never breaks the others (02 §3). */
data class FeedRow(val kind: FeedKind, val state: LoadState<List<Media>>)

data class HomeUiState(
    val feeds: List<FeedRow> = HOME_FEEDS.map { FeedRow(it, LoadState.Loading) },
    val continueWatching: List<HistoryEntry> = emptyList(),
) {
    /** Hero carousel = first 5 trending items. */
    val hero: LoadState<List<Media>>
        get() = when (val s = feeds.firstOrNull { it.kind == HERO_FEED }?.state ?: LoadState.Loading) {
            is LoadState.Ok -> LoadState.Ok(s.value.take(HERO_SIZE))
            else -> s
        }

    /** Rows rendered below the hero (everything but the hero feed). */
    val rows: List<FeedRow> get() = feeds.filter { it.kind != HERO_FEED }
}

val HERO_FEED = FeedKind.TRENDING
const val HERO_SIZE = 5
val HOME_FEEDS = listOf(FeedKind.TRENDING, FeedKind.AIRING, FeedKind.SEASONAL, FeedKind.POPULAR, FeedKind.TOP_RATED)

class HomeViewModel(
    private val metadata: MetadataRepository,
    history: HistoryRepository,
) : ViewModel() {

    private val feeds = MutableStateFlow(HOME_FEEDS.map { FeedRow(it, LoadState.Loading) })

    val state: StateFlow<HomeUiState> = combine(
        feeds,
        history.continueWatching(CONTINUE_LIMIT).catch { emit(emptyList()) },
    ) { rows, continueWatching ->
        HomeUiState(feeds = rows, continueWatching = continueWatching)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), HomeUiState())

    init {
        HOME_FEEDS.forEach(::load)
    }

    /** Reloads a single row (the "Retry" button of its inline error). */
    fun retry(kind: FeedKind) {
        setRow(kind, LoadState.Loading)
        load(kind)
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
