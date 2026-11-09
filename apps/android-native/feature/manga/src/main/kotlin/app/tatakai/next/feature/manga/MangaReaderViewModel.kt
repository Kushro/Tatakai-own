// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.manga.ChapterLists
import app.tatakai.next.manga.ChapterOrder
import app.tatakai.next.manga.MangaCatalog
import app.tatakai.next.manga.MangaRepositoryImpl
import app.tatakai.next.manga.sourceKey
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.MangaRepository
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.ReaderSettings
import app.tatakai.next.model.ReadingProgress
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.toAppError
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** One chapter of the reader with its pages (vertical / webtoon infinite scroll keeps several). */
data class ReaderSegment(val chapter: MangaChapter, val pages: LoadState<List<ChapterPage>>)

data class MangaReaderUi(
    val segments: List<ReaderSegment> = emptyList(),
    /** Index in [segments] of the chapter on screen. */
    val current: Int = 0,
    /** Page on screen inside the current segment. */
    val page: Int = 0,
    val chapters: List<MangaChapter> = emptyList(),
    val source: MangaChapterSource? = null,
    val alternatives: LoadState<List<MangaChapterSource>>? = null,
    val failedPages: Set<String> = emptySet(),
    /** Pages of the next chapter fetched at 70 % (checklist manga.reader.prefetch); the screen warms the image cache with them. */
    val prefetched: List<ChapterPage> = emptyList(),
) {
    val chapter: MangaChapter? get() = segments.getOrNull(current)?.chapter
    val pages: List<ChapterPage> get() = (segments.getOrNull(current)?.pages as? LoadState.Ok)?.value.orEmpty()
}

sealed interface ReaderEvent {
    data class Replace(val args: MangaReaderArgs) : ReaderEvent
    data object NoMatch : ReaderEvent
    data object NoNextChapter : ReaderEvent
    data class Failed(val error: AppError) : ReaderEvent
}

/** Pure reader decisions (JVM-tested): 70 % prefetch threshold and paged spreads. */
object ReaderMath {
    const val PREFETCH_AT: Float = 0.7f

    fun shouldPrefetch(page: Int, total: Int): Boolean = total > 0 && (page + 1).toFloat() / total >= PREFETCH_AT

    /** Double-page spreads: the first page (cover) alone, then pairs. Single page mode = one page per spread. */
    fun spreads(count: Int, doublePage: Boolean): List<List<Int>> {
        if (count <= 0) return emptyList()
        if (!doublePage) return (0 until count).map { listOf(it) }
        val out = mutableListOf(listOf(0))
        var i = 1
        while (i < count) {
            out += if (i + 1 < count) listOf(i, i + 1) else listOf(i)
            i += 2
        }
        return out
    }

    fun spreadOf(spreads: List<List<Int>>, page: Int): Int = spreads.indexOfFirst { page in it }.coerceAtLeast(0)

    /** Pages to warm around [page]: the whole chapter when eager, else the next [ahead] pages. */
    fun warmRange(page: Int, total: Int, eager: Boolean, ahead: Int): IntRange =
        if (eager) 0 until total else (page + 1).coerceAtMost(total) until (page + 1 + ahead).coerceAtMost(total)
}

class MangaReaderViewModel(
    private val args: MangaReaderArgs,
    private val repo: MangaRepository,
    private val catalog: MangaCatalog,
    private val settingsRepo: SettingsRepository,
) : ViewModel() {
    private val _ui = MutableStateFlow(MangaReaderUi())
    val ui: StateFlow<MangaReaderUi> = _ui.asStateFlow()
    val reader: StateFlow<ReaderSettings> = settingsRepo.settings.map { it.reader.sanitized() }
        .stateIn(viewModelScope, SharingStarted.Eagerly, settingsRepo.settings.value.reader.sanitized())
    val settings: StateFlow<AppSettings> = settingsRepo.settings
    private val _events = MutableSharedFlow<ReaderEvent>(extraBufferCapacity = 4)
    val events: SharedFlow<ReaderEvent> = _events.asSharedFlow()

    private val ref: MediaRef = args.anilistId?.let { MediaRef(anilistId = it) } ?: MangaRepositoryImpl.refOf(args.mediaId) ?: MediaRef()
    private val prefetchedFor = mutableSetOf<String>()
    private val nextCache = HashMap<String, List<ChapterPage>>()
    private val reportedFailure = mutableSetOf<String>()
    private var lastSavedAt = 0L
    private val saveScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    init {
        val first = MangaChapter(
            id = args.chapterId,
            extensionId = args.extensionId,
            sourceId = args.sourceId,
            itemId = args.itemId,
            number = args.chapterNumber,
            downloaded = args.offline,
        )
        _ui.value = MangaReaderUi(segments = listOf(ReaderSegment(first, LoadState.Loading)), page = args.startPage.coerceAtLeast(0))
        loadPages(0, first, offline = args.offline)
        viewModelScope.launch { loadChapters() }
        viewModelScope.launch {
            runCatching { catalog.sourceOf(args.extensionId, args.sourceId, args.itemId) }.getOrNull()?.let { s -> _ui.update { it.copy(source = s) } }
        }
    }

    private suspend fun loadChapters() {
        val online = runCatching { catalog.chaptersOf(args.extensionId, args.sourceId, args.itemId) }.getOrNull().orEmpty()
        val offline = runCatching { repo.downloadedChapters(args.mediaId).first() }.getOrNull().orEmpty()
            .filter { it.extensionId == args.extensionId && it.sourceId == args.sourceId && it.itemId == args.itemId }
        val merged = (online.ifEmpty { offline }).map { ch -> if (offline.any { it.id == ch.id }) ch.copy(downloaded = true) else ch }
        _ui.update { ui ->
            val segs = ui.segments.mapIndexed { i, seg ->
                if (i == 0) merged.firstOrNull { it.id == seg.chapter.id }?.let { seg.copy(chapter = it.copy(downloaded = it.downloaded || seg.chapter.downloaded)) } ?: seg else seg
            }
            ui.copy(chapters = merged, segments = segs)
        }
    }

    private fun loadPages(index: Int, chapter: MangaChapter, offline: Boolean) {
        viewModelScope.launch {
            val state = try {
                val cached = nextCache[chapter.id]
                val pages = when {
                    cached != null -> cached
                    offline -> repo.offlinePages(MangaRepositoryImpl.downloadId(chapter)).ifEmpty { repo.offlinePages(chapter.id) }.ifEmpty { repo.pages(chapter) }
                    else -> repo.pages(chapter)
                }
                LoadState.Ok(pages)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            _ui.update { ui ->
                if (index >= ui.segments.size || ui.segments[index].chapter.id != chapter.id) ui
                else ui.copy(segments = ui.segments.toMutableList().also { it[index] = it[index].copy(pages = state) })
            }
            if (state is LoadState.Err && index == 0) _ui.update { it.copy(alternatives = null) }
        }
    }

    fun retry() {
        val ui = _ui.value
        val seg = ui.segments.getOrNull(ui.current) ?: return
        _ui.update { u -> u.copy(failedPages = emptySet(), segments = u.segments.toMutableList().also { it[u.current] = seg.copy(pages = LoadState.Loading) }) }
        nextCache.remove(seg.chapter.id)
        loadPages(ui.current, seg.chapter, offline = seg.chapter.downloaded)
    }

    /** The screen reports the page on screen ([segment] index, [page] index inside it). */
    fun onPage(segment: Int, page: Int) {
        val ui = _ui.value
        if (segment == ui.current && page == ui.page) return
        _ui.update { it.copy(current = segment.coerceIn(0, (it.segments.size - 1).coerceAtLeast(0)), page = page.coerceAtLeast(0)) }
        val total = _ui.value.pages.size
        if (ReaderMath.shouldPrefetch(page, total)) prefetchNext()
        val now = System.currentTimeMillis()
        val last = total > 0 && page >= total - 1
        if (last || now - lastSavedAt > SAVE_EVERY_MS) saveProgress()
    }

    private fun nextChapter(): MangaChapter? {
        val ui = _ui.value
        val current = ui.chapter ?: return null
        return ChapterLists.neighbour(ui.chapters, current, forward = true)
    }

    private fun previousChapter(): MangaChapter? {
        val ui = _ui.value
        val current = ui.chapter ?: return null
        return ChapterLists.neighbour(ui.chapters, current, forward = false)
    }

    val hasNext: Boolean get() = nextChapter() != null
    val hasPrevious: Boolean get() = previousChapter() != null

    private fun prefetchNext() {
        val next = nextChapter() ?: return
        if (!prefetchedFor.add(next.id)) return
        viewModelScope.launch {
            val pages = runCatching { if (next.downloaded) repo.offlinePages(MangaRepositoryImpl.downloadId(next)).ifEmpty { repo.pages(next) } else repo.pages(next) }.getOrNull()
                ?: return@launch
            nextCache[next.id] = pages
            _ui.update { it.copy(prefetched = pages) }
        }
    }

    /** Vertical / webtoon infinite scroll: appends the next chapter below the current one. */
    fun appendNext() {
        val next = nextChapter() ?: return
        val ui = _ui.value
        if (ui.segments.any { it.chapter.id == next.id }) return
        val index = ui.segments.size
        _ui.update { it.copy(segments = it.segments + ReaderSegment(next, LoadState.Loading)) }
        saveProgress()
        loadPages(index, next, offline = next.downloaded)
    }

    fun argsFor(chapter: MangaChapter, startPage: Int = 0): MangaReaderArgs = args.copy(
        extensionId = chapter.extensionId,
        sourceId = chapter.sourceId,
        itemId = chapter.itemId,
        chapterId = chapter.id,
        chapterNumber = chapter.number,
        startPage = startPage,
        offline = chapter.downloaded,
    )

    fun goNext() {
        saveProgress()
        val next = nextChapter()
        if (next == null) _events.tryEmit(ReaderEvent.NoNextChapter) else _events.tryEmit(ReaderEvent.Replace(argsFor(next)))
    }

    fun goPrevious() {
        saveProgress()
        previousChapter()?.let { _events.tryEmit(ReaderEvent.Replace(argsFor(it))) }
    }

    fun open(chapter: MangaChapter) {
        saveProgress()
        _events.tryEmit(ReaderEvent.Replace(argsFor(chapter)))
    }

    /** A page image failed (the reliability of the source follows reality, checklist manga.reader.provider-switch). */
    fun onPageError(url: String) {
        _ui.update { it.copy(failedPages = it.failedPages + url) }
        val chapter = _ui.value.chapter ?: return
        if (reportedFailure.add(chapter.id)) viewModelScope.launch { catalog.reportPageResult(sourceKey(chapter.extensionId, chapter.sourceId), false) }
        if (_ui.value.alternatives == null) loadAlternatives()
    }

    fun onPageLoaded(url: String) {
        if (url in _ui.value.failedPages) _ui.update { it.copy(failedPages = it.failedPages - url) }
    }

    fun loadAlternatives() {
        _ui.update { it.copy(alternatives = LoadState.Loading) }
        viewModelScope.launch {
            val state = try {
                LoadState.Ok(repo.chapterSources(ref, args.title).filter { it.itemId != null })
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            _ui.update { it.copy(alternatives = state) }
        }
    }

    /** Opens the same chapter number in another extension. */
    fun switchSource(source: MangaChapterSource) {
        val number = _ui.value.chapter?.number ?: args.chapterNumber
        viewModelScope.launch {
            val list = try {
                repo.chapters(source)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _events.tryEmit(ReaderEvent.Failed(e.toAppError()))
                return@launch
            }
            val langs = catalog.preferredLanguages()
            val match = if (number == null) {
                ChapterLists.sorted(list, ChapterOrder.ASC).firstOrNull()
            } else {
                list.filter { it.number == number }.minByOrNull { ch -> (ch.lang?.let { l -> langs.indexOf(l) } ?: -1).let { i -> if (i < 0) Int.MAX_VALUE else i } }
            }
            if (match == null) _events.tryEmit(ReaderEvent.NoMatch) else _events.tryEmit(ReaderEvent.Replace(argsFor(match, startPage = _ui.value.page)))
        }
    }

    fun updateReader(transform: (ReaderSettings) -> ReaderSettings) {
        viewModelScope.launch { settingsRepo.update { it.copy(reader = transform(it.reader).sanitized()) } }
    }

    fun saveProgress() {
        val ui = _ui.value
        val chapter = ui.chapter ?: return
        val total = ui.pages.size
        if (total == 0) return
        lastSavedAt = System.currentTimeMillis()
        val progress = ReadingProgress(
            mediaId = args.mediaId,
            title = args.title,
            poster = args.poster,
            chapterId = chapter.id,
            chapterNumber = chapter.number,
            extensionId = chapter.extensionId,
            sourceId = chapter.sourceId,
            itemId = chapter.itemId,
            page = ui.page.coerceIn(0, total - 1),
            totalPages = total,
            updatedAtMs = lastSavedAt,
        )
        // Outlives the screen: closing the reader must still write the position (checklist manga.reader.progress).
        saveScope.launch { runCatching { repo.saveProgress(progress) } }
    }

    override fun onCleared() {
        saveProgress()
        super.onCleared()
    }

    companion object {
        private const val SAVE_EVERY_MS = 3_000L
    }
}
