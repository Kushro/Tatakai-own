// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.manga.MangaCatalog
import app.tatakai.next.manga.MangaInfo
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ContinueReadingItem
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListProgress
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.NovelChapter
import app.tatakai.next.model.NovelHome
import app.tatakai.next.model.NovelProgress
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.NovelSourceSummary
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.toAppError
import app.tatakai.next.novel.NovelChapters
import app.tatakai.next.novel.NovelRepositoryImpl
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

// ------------------------------------------------------------------------------------------------------------------- home

/** Novel search filters (checklist novel.search: genre, status, origin, sort). */
data class NovelFilters(
    val query: String = "",
    val genre: String? = null,
    val status: String? = null,
    val origin: String? = null,
    val sort: MediaSort = MediaSort.POPULARITY,
) {
    val active: Boolean get() = query.isNotBlank() || genre != null || status != null || origin != null

    fun toQuery(): MediaSearchQuery = MediaSearchQuery(
        query = query.trim().ifEmpty { null },
        genres = listOfNotNull(genre),
        status = status,
        sort = if (query.isNotBlank() && sort == MediaSort.POPULARITY) MediaSort.RELEVANCE else sort,
        origin = origin,
        formats = listOf(NovelRepositoryImpl.NOVEL_FORMAT),
    )

    companion object {
        val ORIGINS: List<String> = listOf("JP", "KR", "CN")
        val STATUSES: List<String> = listOf("RELEASING", "FINISHED", "HIATUS", "CANCELLED", "NOT_YET_RELEASED")
        val SORTS: List<MediaSort> = listOf(MediaSort.POPULARITY, MediaSort.TRENDING, MediaSort.SCORE, MediaSort.START_DATE, MediaSort.TITLE, MediaSort.FAVOURITES)
    }
}

data class NovelResultsUi(
    val state: LoadState<List<Media>>? = null,
    val page: Int = 1,
    val hasNext: Boolean = false,
    val loadingMore: Boolean = false,
)

class NovelHomeViewModel(
    private val repo: NovelRepositoryImpl,
    settingsRepo: SettingsRepository,
) : ViewModel() {
    val home: StateFlow<NovelHome> = repo.home().stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), NovelHome())

    /** "Seguir leyendo": saved positions + readlist entries being read (ListProgress.mergeContinueReading). */
    val continueReading: StateFlow<List<ContinueReadingItem>> = combine(repo.lastRead(NOVEL_CONTINUE_LIMIT), repo.readlist(null)) { p, l ->
        novelContinueReading(p, l, NOVEL_CONTINUE_LIMIT)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())
    val settings: StateFlow<AppSettings> = settingsRepo.settings
    private val _filters = MutableStateFlow(NovelFilters())
    val filters: StateFlow<NovelFilters> = _filters.asStateFlow()
    private val _results = MutableStateFlow(NovelResultsUi())
    val results: StateFlow<NovelResultsUi> = _results.asStateFlow()
    private var job: Job? = null
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

    fun setQuery(q: String) = _filters.update { it.copy(query = q) }

    fun setFilters(transform: (NovelFilters) -> NovelFilters) {
        _filters.update(transform)
        if (_filters.value.active) search() else clear()
    }

    fun clear() {
        job?.cancel()
        _filters.value = NovelFilters()
        _results.value = NovelResultsUi()
    }

    fun search() {
        val f = _filters.value
        if (!f.active) return clear()
        job?.cancel()
        _results.value = NovelResultsUi(LoadState.Loading)
        job = viewModelScope.launch {
            _results.value = try {
                val page = repo.search(f.toQuery(), 1)
                NovelResultsUi(LoadState.Ok(page.items), page.page, page.hasNext)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                NovelResultsUi(LoadState.Err(e.toAppError()))
            }
        }
    }

    fun loadMore() {
        val cur = _results.value
        val items = (cur.state as? LoadState.Ok)?.value ?: return
        if (!cur.hasNext || cur.loadingMore) return
        _results.value = cur.copy(loadingMore = true)
        job = viewModelScope.launch {
            _results.value = try {
                val page = repo.search(_filters.value.toQuery(), cur.page + 1)
                NovelResultsUi(LoadState.Ok((items + page.items).distinctBy { it.ref }), page.page, page.hasNext)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                cur.copy(loadingMore = false)
            }
        }
    }

    fun dismissUpdate(mediaId: String) {
        viewModelScope.launch { repo.dismissUpdate(mediaId) }
    }
}

// ------------------------------------------------------------------------------------------------------------------- details

data class NovelDetailsUi(
    val info: LoadState<MangaInfo> = LoadState.Loading,
    val sources: LoadState<List<NovelSourceSummary>> = LoadState.Loading,
    val checking: Boolean = false,
    val source: NovelSourceSummary? = null,
    val chapters: LoadState<List<NovelChapter>> = LoadState.Loading,
    val ascending: Boolean = true,
    val query: String = "",
    val translator: String? = null,
    val lang: String? = null,
    val selecting: Boolean = false,
    val selected: Set<String> = emptySet(),
) {
    val shown: List<NovelChapter>
        get() {
            val list = (chapters as? LoadState.Ok)?.value ?: return emptyList()
            return NovelChapters.filtered(NovelChapters.sorted(list, ascending), query, translator, lang)
        }
}

sealed interface NovelEvent {
    data class Queued(val count: Int) : NovelEvent
    data class Failed(val error: AppError) : NovelEvent
    data class Open(val args: NovelReaderArgs) : NovelEvent
    data object NoChapter : NovelEvent
}

class NovelDetailsViewModel(
    private val anilistId: Int,
    private val repo: NovelRepositoryImpl,
    private val catalog: MangaCatalog,
    settingsRepo: SettingsRepository,
) : ViewModel() {
    private val ref = MediaRef(anilistId = anilistId)
    private val mediaId = NovelRepositoryImpl.mediaKey(ref) ?: anilistId.toString()
    private val _ui = MutableStateFlow(NovelDetailsUi())
    val ui: StateFlow<NovelDetailsUi> = _ui.asStateFlow()
    val settings: StateFlow<AppSettings> = settingsRepo.settings
    val entry: StateFlow<ReadEntry?> = repo.readEntry(ref).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)
    val progress: StateFlow<NovelProgress?> = repo.progress(mediaId).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)
    val downloaded: StateFlow<Set<String>> = repo.downloadedChapters(mediaId).map { l -> l.map(NovelRepositoryImpl::downloadId).toSet() }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptySet())
    private val _events = MutableSharedFlow<NovelEvent>(extraBufferCapacity = 4)
    val events: SharedFlow<NovelEvent> = _events.asSharedFlow()
    private var chaptersJob: Job? = null

    init {
        load()
    }

    private fun media(): Media? = (_ui.value.info as? LoadState.Ok)?.value?.media

    fun load() {
        _ui.update { it.copy(info = LoadState.Loading) }
        viewModelScope.launch {
            val info = try {
                catalog.info(ref)?.let { LoadState.Ok(it) } ?: LoadState.Err(AppError.Http(404))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            _ui.update { it.copy(info = info) }
            if (info is LoadState.Ok) loadSources()
        }
    }

    fun loadSources() {
        val title = media()?.title.orEmpty()
        _ui.update { it.copy(sources = LoadState.Loading) }
        viewModelScope.launch {
            val state = try {
                LoadState.Ok(repo.sources(ref, title))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            applySources(state)
        }
    }

    /** "Comprobar todas" (checklist novel.details.check-sources): every novel extension reports its chapter count. */
    fun checkAll() {
        val title = media()?.title.orEmpty()
        _ui.update { it.copy(checking = true) }
        viewModelScope.launch {
            val state = try {
                LoadState.Ok(repo.checkSources(ref, title))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            _ui.update { it.copy(checking = false) }
            applySources(state)
        }
    }

    private fun applySources(state: LoadState<List<NovelSourceSummary>>) {
        val chosen = (state as? LoadState.Ok)?.value?.let { list ->
            _ui.value.source?.let { cur -> list.firstOrNull { it.extensionId == cur.extensionId && it.sourceId == cur.sourceId } }
                ?: list.firstOrNull { it.itemId != null }
        }
        val changed = chosen?.let { c -> _ui.value.source?.let { it.extensionId != c.extensionId || it.sourceId != c.sourceId } ?: true } ?: false
        _ui.update { it.copy(sources = state, source = chosen ?: it.source) }
        if (chosen == null) {
            _ui.update { it.copy(chapters = LoadState.Ok(emptyList())) }
        } else if (changed || _ui.value.chapters !is LoadState.Ok) {
            loadChapters(chosen, refresh = false)
        }
    }

    fun selectSource(s: NovelSourceSummary) {
        _ui.update { it.copy(source = s, translator = null, lang = null, selected = emptySet()) }
        loadChapters(s, refresh = false)
    }

    fun refreshChapters() {
        _ui.value.source?.let { loadChapters(it, refresh = true) }
    }

    private fun loadChapters(s: NovelSourceSummary, refresh: Boolean) {
        chaptersJob?.cancel()
        _ui.update { it.copy(chapters = LoadState.Loading) }
        chaptersJob = viewModelScope.launch {
            val state = try {
                LoadState.Ok(repo.chapters(s, refresh))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            _ui.update { it.copy(chapters = state) }
        }
    }

    fun setQuery(q: String) = _ui.update { it.copy(query = q) }
    fun setTranslator(t: String?) = _ui.update { it.copy(translator = t) }
    fun setLang(l: String?) = _ui.update { it.copy(lang = l) }
    fun toggleOrder() = _ui.update { it.copy(ascending = !it.ascending) }
    fun setSelecting(on: Boolean) = _ui.update { it.copy(selecting = on, selected = if (on) it.selected else emptySet()) }
    fun toggle(ch: NovelChapter) = _ui.update { ui ->
        val id = NovelRepositoryImpl.downloadId(ch)
        ui.copy(selected = if (id in ui.selected) ui.selected - id else ui.selected + id)
    }
    fun selectAll() = _ui.update { ui -> ui.copy(selected = ui.shown.map(NovelRepositoryImpl::downloadId).toSet()) }

    fun downloadSelected() {
        val ui = _ui.value
        val media = media() ?: return
        val list = ((ui.chapters as? LoadState.Ok)?.value ?: emptyList()).filter { NovelRepositoryImpl.downloadId(it) in ui.selected }
        if (list.isEmpty()) return
        viewModelScope.launch {
            repo.downloadChapters(media, list)
                .onSuccess { _events.tryEmit(NovelEvent.Queued(it)) }
                .onFailure { _events.tryEmit(NovelEvent.Failed(it.toAppError())) }
            _ui.update { it.copy(selecting = false, selected = emptySet()) }
        }
    }

    fun setStatus(status: ListStatus?) {
        val media = media() ?: return
        viewModelScope.launch { repo.setReadStatus(media, status).onFailure { _events.tryEmit(NovelEvent.Failed(it.toAppError())) } }
    }

    fun setProgress(chapters: Int) {
        viewModelScope.launch { repo.setReadProgress(ref, chapters.coerceAtLeast(0)).onFailure { _events.tryEmit(NovelEvent.Failed(it.toAppError())) } }
    }

    fun argsFor(ch: NovelChapter, startParagraph: Int = 0, offline: Boolean = ch.downloaded): NovelReaderArgs {
        val media = media()
        return NovelReaderArgs(
            mediaId = mediaId,
            title = media?.title.orEmpty(),
            poster = media?.cover,
            extensionId = ch.extensionId,
            sourceId = ch.sourceId,
            itemId = ch.itemId,
            chapterId = ch.id,
            chapterNumber = ch.number,
            startParagraph = startParagraph,
            offline = offline,
            anilistId = anilistId,
        )
    }

    /** "Leer" / "Continuar": the saved position, else the first unread chapter of the selected source. */
    fun readNext() {
        val p = progress.value
        val listProgress = entry.value?.progress ?: 0
        // the saved paragraph wins unless the list (AniList) is already past that chapter (ListProgress.resumeSaved)
        if (p != null && ListProgress.resumeSaved(p.chapterNumber, p.offset >= NovelRepositoryImpl.COMPLETED_OFFSET, listProgress)) {
            _events.tryEmit(NovelEvent.Open(NovelReaderArgs(mediaId, p.title, p.poster, p.extensionId, p.sourceId, p.itemId, p.chapterId, p.chapterNumber, p.paragraph, anilistId = anilistId)))
            return
        }
        val asc = NovelChapters.sorted((_ui.value.chapters as? LoadState.Ok)?.value.orEmpty(), ascending = true)
        val read = ListProgress.readThrough(p?.chapterNumber, listProgress)
        val next = asc.firstOrNull { (it.number ?: 0.0) > read } ?: asc.firstOrNull()
        if (next == null) _events.tryEmit(NovelEvent.NoChapter) else _events.tryEmit(NovelEvent.Open(argsFor(next)))
    }

    /** The chapter number "Leer capítulo N" opens (same rule as [readNext]); null = unknown. */
    fun nextChapterNumber(p: NovelProgress?, e: ReadEntry?, chapters: List<NovelChapter>): Double? {
        val listProgress = e?.progress ?: 0
        if (p != null && ListProgress.resumeSaved(p.chapterNumber, p.offset >= NovelRepositoryImpl.COMPLETED_OFFSET, listProgress)) return p.chapterNumber
        val read = ListProgress.readThrough(p?.chapterNumber, listProgress)
        val asc = NovelChapters.sorted(chapters, ascending = true)
        return asc.firstOrNull { (it.number ?: 0.0) > read }?.number ?: (read.toInt() + 1).toDouble().takeIf { chapters.isEmpty() && read > 0 }
    }

    /** Chapter row toggle (ListProgress): read sets P = max(P, n) (adds the title as Leyendo when needed), not read sets P = min(P, n - 1). */
    fun toggleRead(ch: NovelChapter) {
        val n = ListProgress.chapterIndex(ch.number) ?: return
        val media = media() ?: return
        val current = entry.value
        val read = ListProgress.isChapterRead(ch.number, current?.progress ?: 0)
        viewModelScope.launch {
            val result = if (read) {
                // a finished title becomes Leyendo again (P = n - 1 < T)
                val reopened = ListProgress.statusAfterUnmark(current?.status)?.let { repo.setReadStatus(media, it) } ?: Result.success(Unit)
                reopened.fold(
                    onSuccess = { repo.setReadProgress(ref, ListProgress.toggledProgress(current?.progress ?: 0, n, watched = false)) },
                    onFailure = { Result.failure(it) },
                )
            } else {
                val added = if (current == null) repo.setReadStatus(media, ListStatus.WATCHING) else Result.success(Unit)
                added.fold(
                    onSuccess = { repo.setReadProgress(ref, ListProgress.toggledProgress(current?.progress ?: 0, n, watched = true, total = current?.totalChapters ?: media.episodes)) },
                    onFailure = { Result.failure(it) },
                )
            }
            result.onFailure { _events.tryEmit(NovelEvent.Failed(it.toAppError())) }
        }
    }
}

// ------------------------------------------------------------------------------------------------------------------- library

class NovelLibraryViewModel(
    private val repo: NovelRepositoryImpl,
    lists: ListsRepository?,
) : ViewModel() {
    val entries: StateFlow<List<ReadEntry>?> = repo.readlist(null).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)
    val lastRead: StateFlow<List<NovelProgress>> = repo.lastRead(10).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())

    /** "Seguir leyendo": saved positions + readlist entries being read (ListProgress.mergeContinueReading). */
    val continueReading: StateFlow<List<ContinueReadingItem>> = combine(repo.lastRead(10), repo.readlist(null)) { p, l ->
        novelContinueReading(p, l, 10)
    }.stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())
    val linked: StateFlow<Boolean> = (lists?.accounts?.map { a -> a.any { it.provider == ListProvider.ANILIST } } ?: flowOf(false))
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), false)
    private val _syncing = MutableStateFlow(false)
    val syncing: StateFlow<Boolean> = _syncing.asStateFlow()

    fun sync() {
        if (_syncing.value) return
        _syncing.value = true
        viewModelScope.launch {
            runCatching { repo.pullReadlist() }
            _syncing.value = false
        }
    }
}

private const val NOVEL_CONTINUE_LIMIT = 20
