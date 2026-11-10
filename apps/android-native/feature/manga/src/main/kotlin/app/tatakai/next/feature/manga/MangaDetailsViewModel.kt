// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.manga.ChapterLists
import app.tatakai.next.manga.ChapterOrder
import app.tatakai.next.manga.MangaCatalog
import app.tatakai.next.manga.MangaInfo
import app.tatakai.next.manga.MangaPrefs
import app.tatakai.next.manga.MangaRepositoryImpl
import app.tatakai.next.manga.MangaUpdatesSeries
import app.tatakai.next.manga.key
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ListProgress
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import app.tatakai.next.model.MangaDetails
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.MangaRepository
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.model.ReadingProgress
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.toAppError
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Chapter list tools of the details page (checklist manga.chapters.tools). */
data class ChapterTools(
    val order: ChapterOrder = ChapterOrder.DESC,
    val query: String = "",
    val lang: String? = null,
    val byVolume: Boolean = false,
)

data class MangaDetailsUi(
    val info: LoadState<MangaInfo> = LoadState.Loading,
    val details: MangaDetails? = null,
    val mangaUpdates: MangaUpdatesSeries? = null,
    val sources: LoadState<List<MangaChapterSource>> = LoadState.Loading,
    val source: MangaChapterSource? = null,
    val chapters: LoadState<List<MangaChapter>> = LoadState.Loading,
    val tools: ChapterTools = ChapterTools(),
    val selecting: Boolean = false,
    val selected: Set<String> = emptySet(),
    val opening: Boolean = false,
) {
    val shown: List<MangaChapter>
        get() {
            val list = (chapters as? LoadState.Ok)?.value ?: return emptyList()
            return ChapterLists.filtered(ChapterLists.sorted(list, tools.order), tools.query, tools.lang)
        }
}

sealed interface DetailsEvent {
    data class Queued(val count: Int) : DetailsEvent
    data class Failed(val error: AppError) : DetailsEvent
    data object NoChapter : DetailsEvent
    data class Open(val args: MangaReaderArgs) : DetailsEvent
}

class MangaDetailsViewModel(
    private val anilistId: Int,
    private val repo: MangaRepository,
    private val catalog: MangaCatalog,
    settingsRepo: SettingsRepository,
) : ViewModel() {
    private val ref = MediaRef(anilistId = anilistId)
    private val mediaId: String = MangaRepositoryImpl.mediaKey(ref) ?: anilistId.toString()

    private val _ui = MutableStateFlow(MangaDetailsUi())
    val ui: StateFlow<MangaDetailsUi> = _ui.asStateFlow()
    val settings: StateFlow<AppSettings> = settingsRepo.settings
    val prefs: StateFlow<MangaPrefs> = catalog.prefs
    val entry: StateFlow<ReadEntry?> = repo.readEntry(ref).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)
    val progress: StateFlow<ReadingProgress?> = repo.progress(mediaId).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)
    val downloaded: StateFlow<List<MangaChapter>> = repo.downloadedChapters(mediaId).stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), emptyList())
    private val _events = MutableSharedFlow<DetailsEvent>(extraBufferCapacity = 4)
    val events: SharedFlow<DetailsEvent> = _events.asSharedFlow()
    private var chaptersJob: Job? = null

    init {
        load()
    }

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
            if (info is LoadState.Ok) {
                launch { runCatching { repo.details(ref) }.getOrNull()?.let { d -> _ui.update { it.copy(details = d) } } }
                launch { runCatching { catalog.mangaUpdates(ref) }.getOrNull()?.let { mu -> _ui.update { it.copy(mangaUpdates = mu) } } }
                loadSources()
            }
        }
    }

    private fun title(): String = ((_ui.value.info as? LoadState.Ok)?.value?.media?.title).orEmpty()

    fun loadSources() {
        _ui.update { it.copy(sources = LoadState.Loading) }
        viewModelScope.launch {
            val state = try {
                LoadState.Ok(repo.chapterSources(ref, title()))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            val chosen = (state as? LoadState.Ok)?.value?.let { list ->
                val keep = _ui.value.source?.let { cur -> list.firstOrNull { it.key == cur.key } }
                keep ?: list.firstOrNull { it.itemId != null } ?: list.firstOrNull()
            }
            _ui.update { it.copy(sources = state, source = chosen) }
            if (chosen != null) loadChapters(chosen, refresh = false) else _ui.update { it.copy(chapters = LoadState.Ok(emptyList())) }
        }
    }

    fun selectSource(source: MangaChapterSource) {
        _ui.update { it.copy(source = source, selected = emptySet(), tools = it.tools.copy(lang = null)) }
        loadChapters(source, refresh = false)
    }

    /** "Fuente preferida" for this title (manual override of the automatic choice). */
    fun setPreferredSource(source: MangaChapterSource?) {
        viewModelScope.launch {
            catalog.updatePrefs { p ->
                if (source == null) p.copy(preferredSourceByMedia = p.preferredSourceByMedia - mediaId) else p.copy(preferredSourceByMedia = p.preferredSourceByMedia + (mediaId to source.key))
            }
        }
    }

    fun setLanguages(languages: List<String>) {
        viewModelScope.launch {
            catalog.updatePrefs { it.copy(languages = languages) }
            loadSources()
        }
    }

    fun refreshChapters() {
        _ui.value.source?.let { loadChapters(it, refresh = true) }
    }

    /** "Vaciar caché": forgets matches and chapter lists of this title, then searches the sources again. */
    fun clearCache() {
        viewModelScope.launch {
            runCatching { repo.clearChapterCache(ref) }
            loadSources()
        }
    }

    private fun loadChapters(source: MangaChapterSource, refresh: Boolean) {
        chaptersJob?.cancel()
        _ui.update { it.copy(chapters = LoadState.Loading) }
        chaptersJob = viewModelScope.launch {
            val state = try {
                LoadState.Ok(repo.chapters(source, refresh))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            _ui.update { ui -> if (ui.source?.key == source.key) ui.copy(chapters = state) else ui }
        }
    }

    fun setTools(transform: (ChapterTools) -> ChapterTools) = _ui.update { it.copy(tools = transform(it.tools)) }

    fun setSelecting(on: Boolean) = _ui.update { it.copy(selecting = on, selected = if (on) it.selected else emptySet()) }

    fun toggle(chapter: MangaChapter) = _ui.update { ui ->
        val id = MangaRepositoryImpl.downloadId(chapter)
        ui.copy(selected = if (id in ui.selected) ui.selected - id else ui.selected + id)
    }

    fun selectAllShown() = _ui.update { ui -> ui.copy(selected = ui.shown.map(MangaRepositoryImpl::downloadId).toSet()) }

    fun downloadSelected() {
        val ui = _ui.value
        val media = (ui.info as? LoadState.Ok)?.value?.media ?: return
        val chosen = ((ui.chapters as? LoadState.Ok)?.value ?: emptyList()).filter { MangaRepositoryImpl.downloadId(it) in ui.selected }
        if (chosen.isEmpty()) return
        viewModelScope.launch {
            repo.downloadChapters(media, chosen)
                .onSuccess { n -> _events.tryEmit(DetailsEvent.Queued(n)) }
                .onFailure { e -> _events.tryEmit(DetailsEvent.Failed(e.toAppError())) }
            _ui.update { it.copy(selecting = false, selected = emptySet()) }
        }
    }

    fun setStatus(status: ListStatus?) {
        val media = (_ui.value.info as? LoadState.Ok)?.value?.media ?: return
        viewModelScope.launch {
            repo.setReadStatus(media, status).onFailure { e -> _events.tryEmit(DetailsEvent.Failed(e.toAppError())) }
        }
    }

    fun setProgress(chapters: Int) {
        viewModelScope.launch {
            repo.setReadProgress(ref, chapters.coerceAtLeast(0)).onFailure { e -> _events.tryEmit(DetailsEvent.Failed(e.toAppError())) }
        }
    }

    /**
     * Chapter row toggle (ListProgress, same rules as the episodes): read sets P = max(P, n) (adds the title as Leyendo when it is not
     * on the readlist; reaching the total completes it), not read sets P = min(P, n - 1). Through the readlist (AniList when linked).
     */
    fun toggleRead(chapter: MangaChapter) {
        val n = ListProgress.chapterIndex(chapter.number) ?: return
        val media = (_ui.value.info as? LoadState.Ok)?.value?.media ?: return
        val current = entry.value
        val read = ListProgress.isChapterRead(chapter.number, current?.progress ?: 0)
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
            result.onFailure { e -> _events.tryEmit(DetailsEvent.Failed(e.toAppError())) }
        }
    }

    /** The chapter number "Leer capítulo N" opens: the saved one when it wins, else the first after max(P, saved); null = unknown. */
    fun nextChapterNumber(p: ReadingProgress?, e: ReadEntry?, chapters: List<MangaChapter>): Double? {
        val listProgress = e?.progress ?: 0
        if (p != null && ListProgress.resumeSaved(p.chapterNumber, p.completed, listProgress)) return p.chapterNumber
        val read = ListProgress.readThrough(p?.chapterNumber, listProgress)
        val asc = ChapterLists.sorted(chapters, ChapterOrder.ASC)
        return asc.firstOrNull { (it.number ?: 0.0) > read }?.number ?: (read.toInt() + 1).toDouble().takeIf { chapters.isEmpty() && read > 0 }
    }

    fun acceptMature() {
        viewModelScope.launch { catalog.updatePrefs { it.copy(matureAccepted = it.matureAccepted + anilistId) } }
    }

    fun argsFor(chapter: MangaChapter, startPage: Int = 0): MangaReaderArgs {
        val media = (_ui.value.info as? LoadState.Ok)?.value?.media
        return MangaReaderArgs(
            mediaId = mediaId,
            title = media?.title ?: title(),
            poster = media?.cover,
            extensionId = chapter.extensionId,
            sourceId = chapter.sourceId,
            itemId = chapter.itemId,
            chapterId = chapter.id,
            chapterNumber = chapter.number,
            startPage = startPage,
            offline = chapter.downloaded,
            anilistId = anilistId,
        )
    }

    /**
     * "Leer" / "Continuar" (checklist manga.chapters.auto-best): resumes the saved chapter, else opens the next unread chapter number with
     * the best source for the preferred languages and reliability (`MangaRepository.bestChapter`); falls back to the selected source.
     */
    fun readNext() {
        val p = progress.value
        val listProgress = entry.value?.progress ?: 0
        // the saved page wins unless the list (AniList) is already past that chapter (ListProgress.resumeSaved)
        if (p != null && ListProgress.resumeSaved(p.chapterNumber, p.completed, listProgress)) {
            _events.tryEmit(
                DetailsEvent.Open(
                    MangaReaderArgs(mediaId, p.title, p.poster, p.extensionId, p.sourceId, p.itemId, p.chapterId, p.chapterNumber, p.page, anilistId = anilistId),
                ),
            )
            return
        }
        val list = (_ui.value.chapters as? LoadState.Ok)?.value.orEmpty()
        val asc = ChapterLists.sorted(list, ChapterOrder.ASC)
        val read = ListProgress.readThrough(p?.chapterNumber, listProgress)
        val wanted = asc.firstOrNull { (it.number ?: 0.0) > read }?.number ?: asc.firstOrNull()?.number
        _ui.update { it.copy(opening = true) }
        viewModelScope.launch {
            val best = wanted?.let { n -> runCatching { repo.bestChapter(ref, title(), n) }.getOrNull() }
            val fallback = best ?: asc.firstOrNull { it.number == wanted } ?: asc.firstOrNull()
            _ui.update { it.copy(opening = false) }
            if (fallback == null) _events.tryEmit(DetailsEvent.NoChapter) else _events.tryEmit(DetailsEvent.Open(argsFor(fallback)))
        }
    }
}
