package app.tatakai.next.feature.search

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
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
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

sealed interface SearchResults {
    /** Nothing typed yet: show the suggestions / empty prompt. */
    data object Idle : SearchResults

    data object Loading : SearchResults

    /** Query returned no items. */
    data object Empty : SearchResults

    data class Ok(val items: List<Media>, val page: Int, val hasNext: Boolean, val loadingMore: Boolean = false) : SearchResults

    data class Err(val cause: AppError) : SearchResults
}

data class SearchUiState(
    val query: String = "",
    val results: SearchResults = SearchResults.Idle,
)

@OptIn(FlowPreview::class)
class SearchViewModel(
    private val metadata: MetadataRepository,
    debounceMillis: Long = DEFAULT_DEBOUNCE_MS,
) : ViewModel() {

    private val _state = MutableStateFlow(SearchUiState())
    val state: StateFlow<SearchUiState> = _state.asStateFlow()

    private val queries = MutableStateFlow("")
    private var loadMoreJob: Job? = null

    init {
        viewModelScope.launch {
            queries
                .debounce(debounceMillis)
                .map { it.trim() }
                .distinctUntilChanged()
                .collectLatest { q -> runSearch(q) }
        }
    }

    fun onQueryChange(value: String) {
        _state.update { it.copy(query = value) }
        queries.value = value
    }

    fun clear() = onQueryChange("")

    /** Re-runs the current query immediately (Retry button). */
    fun retry() {
        val q = _state.value.query.trim()
        viewModelScope.launch { runSearch(q) }
    }

    fun loadMore() {
        val current = _state.value.results as? SearchResults.Ok ?: return
        if (!current.hasNext || current.loadingMore) return
        val q = _state.value.query.trim()
        _state.update { it.copy(results = current.copy(loadingMore = true)) }
        loadMoreJob?.cancel()
        loadMoreJob = viewModelScope.launch {
            runCatchingApp { metadata.search(MediaKind.ANIME, q, current.page + 1) }
                .onSuccess { page ->
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
                .onFailure { _state.update { s -> s.copy(results = current.copy(loadingMore = false)) } }
        }
    }

    private suspend fun runSearch(q: String) {
        loadMoreJob?.cancel()
        if (q.isEmpty()) {
            _state.update { it.copy(results = SearchResults.Idle) }
            return
        }
        _state.update { it.copy(results = SearchResults.Loading) }
        val result = runCatchingApp { metadata.search(MediaKind.ANIME, q, 1) }
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

    companion object {
        const val DEFAULT_DEBOUNCE_MS = 350L
    }
}
