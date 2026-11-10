// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.manga.MangaCatalog
import app.tatakai.next.manga.MangaPrefs
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ContinueReadingItem
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MangaHome
import app.tatakai.next.model.MangaRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.toAppError
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Search filters of the manga home (checklist manga.search: origin JP/KR/CN, status, genre, year, sort). */
data class MangaFilters(
    val query: String = "",
    val origin: String? = null,
    val status: String? = null,
    val genre: String? = null,
    val year: Int? = null,
    val sort: MediaSort = MediaSort.POPULARITY,
) {
    val active: Boolean get() = query.isNotBlank() || origin != null || status != null || genre != null || year != null

    fun toQuery(adult: Boolean = false): MediaSearchQuery = MediaSearchQuery(
        query = query.trim().ifEmpty { null },
        genres = listOfNotNull(genre),
        status = status,
        yearFrom = year,
        yearTo = year,
        sort = if (query.isNotBlank() && sort == MediaSort.POPULARITY) MediaSort.RELEVANCE else sort,
        adult = adult,
        origin = origin,
    )

    companion object {
        val ORIGINS: List<String> = listOf("JP", "KR", "CN")
        val STATUSES: List<String> = listOf("RELEASING", "FINISHED", "HIATUS", "CANCELLED", "NOT_YET_RELEASED")
        val SORTS: List<MediaSort> = listOf(MediaSort.POPULARITY, MediaSort.TRENDING, MediaSort.SCORE, MediaSort.START_DATE, MediaSort.TITLE, MediaSort.FAVOURITES)
    }
}

/** Results of a manga / novel search (shared shape with the novel home). */
data class SearchResultsUi(
    val state: LoadState<List<Media>>? = null,
    val page: Int = 1,
    val hasNext: Boolean = false,
    val loadingMore: Boolean = false,
    val moreError: AppError? = null,
)

class MangaHomeViewModel(
    private val repo: MangaRepository,
    private val catalog: MangaCatalog,
    settingsRepo: SettingsRepository,
) : ViewModel() {
    val home: StateFlow<MangaHome> = repo.home().stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), MangaHome())

    /** "Seguir leyendo": [MangaHome.lastRead] merged with the readlist entries being read (ListProgress.mergeContinueReading). */
    val continueReading: StateFlow<List<ContinueReadingItem>> = combine(repo.lastRead(HOME_CONTINUE_LIMIT), repo.readlist(null)) { p, l ->
        mangaContinueReading(p, l, HOME_CONTINUE_LIMIT)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())
    val settings: StateFlow<AppSettings> = settingsRepo.settings
    val prefs: StateFlow<MangaPrefs> = catalog.prefs

    private val _filters = MutableStateFlow(MangaFilters())
    val filters: StateFlow<MangaFilters> = _filters.asStateFlow()
    private val _results = MutableStateFlow(SearchResultsUi())
    val results: StateFlow<SearchResultsUi> = _results.asStateFlow()
    private var searchJob: Job? = null
    private var initialApplied: String? = null

    init {
        refresh()
    }

    fun refresh() {
        viewModelScope.launch { runCatching { repo.refreshHome() } }
    }

    fun applyInitial(query: String?) {
        if (query.isNullOrBlank() || initialApplied == query) return
        initialApplied = query
        _filters.update { it.copy(query = query) }
        search()
    }

    fun setQuery(query: String) = _filters.update { it.copy(query = query) }

    fun setFilters(transform: (MangaFilters) -> MangaFilters) {
        _filters.update(transform)
        if (_filters.value.active) search() else clearSearch()
    }

    fun clearSearch() {
        searchJob?.cancel()
        _filters.value = MangaFilters()
        _results.value = SearchResultsUi()
    }

    fun search() {
        val f = _filters.value
        if (!f.active) {
            _results.value = SearchResultsUi()
            return
        }
        searchJob?.cancel()
        _results.value = SearchResultsUi(state = LoadState.Loading)
        searchJob = viewModelScope.launch {
            _results.value = try {
                val page = repo.search(f.toQuery(), 1)
                SearchResultsUi(LoadState.Ok(page.items), page.page, page.hasNext)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                SearchResultsUi(LoadState.Err(e.toAppError()))
            }
        }
    }

    fun loadMore() {
        val current = _results.value
        val items = (current.state as? LoadState.Ok)?.value ?: return
        if (!current.hasNext || current.loadingMore) return
        _results.value = current.copy(loadingMore = true, moreError = null)
        searchJob = viewModelScope.launch {
            _results.value = try {
                val page = repo.search(_filters.value.toQuery(), current.page + 1)
                SearchResultsUi(LoadState.Ok((items + page.items).distinctBy { it.ref }), page.page, page.hasNext)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                current.copy(loadingMore = false, moreError = e.toAppError())
            }
        }
    }

    /** "Ver zona 18+": the gate was accepted (once) and the zone loads (only with `showAdult`). */
    fun acceptAdultGate() {
        viewModelScope.launch {
            catalog.updatePrefs { it.copy(adultGateAccepted = true) }
            runCatching { catalog.loadAdultZone() }
        }
    }

    fun loadAdultZone() {
        viewModelScope.launch { runCatching { catalog.loadAdultZone() } }
    }

    fun hideAdultZone() {
        viewModelScope.launch { catalog.updatePrefs { it.copy(adultGateAccepted = false) }; runCatching { catalog.loadAdultZone() } }
    }

    fun acceptMature(anilistId: Int) {
        viewModelScope.launch { catalog.updatePrefs { it.copy(matureAccepted = it.matureAccepted + anilistId) } }
    }

    fun dismissUpdate(mediaId: String) {
        viewModelScope.launch { catalog.dismissUpdate(mediaId) }
    }
}

private const val HOME_CONTINUE_LIMIT = 20
