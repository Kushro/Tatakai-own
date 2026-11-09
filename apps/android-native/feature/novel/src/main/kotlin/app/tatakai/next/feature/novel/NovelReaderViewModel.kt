// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Narrator
import app.tatakai.next.model.NovelChapter
import app.tatakai.next.model.NovelChapterText
import app.tatakai.next.model.NovelProgress
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.NovelReaderSettings
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.TtsState
import app.tatakai.next.model.toAppError
import app.tatakai.next.novel.NarratorImpl
import app.tatakai.next.novel.NovelChapters
import app.tatakai.next.novel.NovelRepositoryImpl
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
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class NovelSegment(val chapter: NovelChapter, val text: LoadState<NovelChapterText>)

data class NovelReaderUi(
    val segments: List<NovelSegment> = emptyList(),
    val current: Int = 0,
    val paragraph: Int = 0,
    val chapters: List<NovelChapter> = emptyList(),
    /** Segment being narrated (the narrator reads one chapter at a time). */
    val narrating: Int? = null,
) {
    val chapter: NovelChapter? get() = segments.getOrNull(current)?.chapter
    val paragraphs: List<String> get() = (segments.getOrNull(current)?.text as? LoadState.Ok)?.value?.paragraphs.orEmpty()

    /** 0..1 read inside the current chapter. */
    val fraction: Float get() = paragraphs.size.let { n -> if (n <= 0) 0f else ((paragraph + 1).toFloat() / n).coerceIn(0f, 1f) }
}

sealed interface NovelReaderEvent {
    data class Replace(val args: NovelReaderArgs) : NovelReaderEvent
    data object LastChapter : NovelReaderEvent
}

class NovelReaderViewModel(
    private val args: NovelReaderArgs,
    private val repo: NovelRepositoryImpl,
    private val narratorApi: Narrator,
    private val settingsRepo: SettingsRepository,
) : ViewModel() {
    private val _ui = MutableStateFlow(NovelReaderUi())
    val ui: StateFlow<NovelReaderUi> = _ui.asStateFlow()
    val settings: StateFlow<AppSettings> = settingsRepo.settings
    val reader: StateFlow<NovelReaderSettings> = settingsRepo.settings.map { it.novelReader.sanitized() }
        .stateIn(viewModelScope, SharingStarted.Eagerly, settingsRepo.settings.value.novelReader.sanitized())
    val ttsState: StateFlow<TtsState> = narratorApi.state
    val ttsParagraph: StateFlow<Int?> = narratorApi.currentParagraph
    val ttsError: StateFlow<Throwable?> = (narratorApi as? NarratorImpl)?.lastError ?: MutableStateFlow<Throwable?>(null)
    private val _events = MutableSharedFlow<NovelReaderEvent>(extraBufferCapacity = 4)
    val events: SharedFlow<NovelReaderEvent> = _events.asSharedFlow()
    private val saveScope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var lastSavedAt = 0L

    init {
        val first = NovelChapter(args.chapterId, args.extensionId, args.sourceId, args.itemId, args.chapterNumber, downloaded = args.offline)
        _ui.value = NovelReaderUi(segments = listOf(NovelSegment(first, LoadState.Loading)), paragraph = args.startParagraph.coerceAtLeast(0))
        loadText(0, first)
        viewModelScope.launch {
            val online = runCatching { repo.chaptersOf(args.extensionId, args.sourceId, args.itemId) }.getOrNull().orEmpty()
            val offline = runCatching { repo.downloadedChapters(args.mediaId).first() }.getOrNull().orEmpty()
                .filter { it.extensionId == args.extensionId && it.sourceId == args.sourceId && it.itemId == args.itemId }
            val merged = online.ifEmpty { offline }
            _ui.update { ui ->
                ui.copy(
                    chapters = merged,
                    segments = ui.segments.mapIndexed { i, s -> if (i == 0) merged.firstOrNull { it.id == s.chapter.id }?.let { c -> s.copy(chapter = c) } ?: s else s },
                )
            }
        }
        // Narration continues into the next chapter when it reaches the end of the one it was reading.
        viewModelScope.launch {
            var previous = narratorApi.state.value
            narratorApi.state.collect { st ->
                val ui = _ui.value
                val narrating = ui.narrating
                if (previous == TtsState.SPEAKING && st == TtsState.IDLE && narrating != null && ttsError.value == null) {
                    continueNarration(narrating)
                }
                previous = st
            }
        }
    }

    private fun loadText(index: Int, chapter: NovelChapter) {
        viewModelScope.launch {
            val state = try {
                val offline = if (chapter.downloaded || args.offline) repo.offlineText(NovelRepositoryImpl.downloadId(chapter)) else null
                LoadState.Ok(offline ?: repo.text(chapter))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                LoadState.Err(e.toAppError())
            }
            _ui.update { ui ->
                if (index >= ui.segments.size || ui.segments[index].chapter.id != chapter.id) ui
                else ui.copy(segments = ui.segments.toMutableList().also { it[index] = it[index].copy(text = state) })
            }
        }
    }

    fun retry() {
        val ui = _ui.value
        val seg = ui.segments.getOrNull(ui.current) ?: return
        _ui.update { u -> u.copy(segments = u.segments.toMutableList().also { it[u.current] = seg.copy(text = LoadState.Loading) }) }
        loadText(ui.current, seg.chapter)
    }

    fun onPosition(segment: Int, paragraph: Int) {
        val ui = _ui.value
        if (segment == ui.current && paragraph == ui.paragraph) return
        _ui.update { it.copy(current = segment.coerceIn(0, (it.segments.size - 1).coerceAtLeast(0)), paragraph = paragraph.coerceAtLeast(0)) }
        val now = System.currentTimeMillis()
        if (now - lastSavedAt > SAVE_EVERY_MS || _ui.value.fraction >= NovelRepositoryImpl.COMPLETED_OFFSET) saveProgress()
    }

    private fun neighbour(forward: Boolean): NovelChapter? {
        val ui = _ui.value
        val current = ui.chapter ?: return null
        return NovelChapters.neighbour(ui.chapters, current.id, forward)
    }

    val hasNext: Boolean get() = neighbour(true) != null
    val hasPrevious: Boolean get() = neighbour(false) != null

    /** Continuous scroll (checklist novel.reader.flow): the next chapter is appended below. */
    fun appendNext(): Boolean {
        val next = neighbour(true) ?: return false
        val ui = _ui.value
        if (ui.segments.any { it.chapter.id == next.id }) return true
        val index = ui.segments.size
        _ui.update { it.copy(segments = it.segments + NovelSegment(next, LoadState.Loading)) }
        loadText(index, next)
        return true
    }

    private fun argsFor(ch: NovelChapter): NovelReaderArgs = args.copy(
        extensionId = ch.extensionId,
        sourceId = ch.sourceId,
        itemId = ch.itemId,
        chapterId = ch.id,
        chapterNumber = ch.number,
        startParagraph = 0,
        offline = ch.downloaded,
    )

    fun goNext() {
        saveProgress()
        val next = neighbour(true)
        if (next == null) _events.tryEmit(NovelReaderEvent.LastChapter) else _events.tryEmit(NovelReaderEvent.Replace(argsFor(next)))
    }

    fun goPrevious() {
        saveProgress()
        neighbour(false)?.let { _events.tryEmit(NovelReaderEvent.Replace(argsFor(it))) }
    }

    fun open(ch: NovelChapter) {
        saveProgress()
        _events.tryEmit(NovelReaderEvent.Replace(argsFor(ch)))
    }

    // ---- narration (checklist novel.tts.system / .controls) ----

    fun speak(segment: Int = _ui.value.current, fromParagraph: Int = _ui.value.paragraph) {
        val text = (_ui.value.segments.getOrNull(segment)?.text as? LoadState.Ok)?.value ?: return
        _ui.update { it.copy(narrating = segment) }
        viewModelScope.launch { narratorApi.speak(text, fromParagraph.coerceIn(0, (text.paragraphs.size - 1).coerceAtLeast(0))) }
    }

    private fun continueNarration(finished: Int) {
        val ui = _ui.value
        val next = finished + 1
        if (next < ui.segments.size) {
            if (ui.segments[next].text is LoadState.Ok) speak(next, 0) else _ui.update { it.copy(narrating = null) }
            return
        }
        if (appendNext()) {
            viewModelScope.launch {
                // Waits for the appended chapter text, then keeps reading.
                val text = _ui.map { it.segments.getOrNull(next)?.text }.first { it is LoadState.Ok || it is LoadState.Err }
                if (text is LoadState.Ok) speak(next, 0) else _ui.update { it.copy(narrating = null) }
            }
        } else {
            _ui.update { it.copy(narrating = null) }
        }
    }

    fun pause() = narratorApi.pause()
    fun resume() = narratorApi.resume()
    fun stopNarration() {
        _ui.update { it.copy(narrating = null) }
        narratorApi.stop()
    }

    fun setSpeed(speed: Float) {
        narratorApi.setSpeed(speed)
        viewModelScope.launch { settingsRepo.update { it.copy(tts = it.tts.copy(speed = speed).sanitized()) } }
    }

    fun updateReader(transform: (NovelReaderSettings) -> NovelReaderSettings) {
        viewModelScope.launch { settingsRepo.update { it.copy(novelReader = transform(it.novelReader).sanitized()) } }
    }

    fun saveProgress() {
        val ui = _ui.value
        val chapter = ui.chapter ?: return
        if (ui.paragraphs.isEmpty()) return
        lastSavedAt = System.currentTimeMillis()
        val p = NovelProgress(
            mediaId = args.mediaId,
            title = args.title,
            poster = args.poster,
            chapterId = chapter.id,
            chapterNumber = chapter.number,
            extensionId = chapter.extensionId,
            sourceId = chapter.sourceId,
            itemId = chapter.itemId,
            paragraph = ui.paragraph.coerceIn(0, ui.paragraphs.size - 1),
            offset = ui.fraction,
            updatedAtMs = lastSavedAt,
        )
        saveScope.launch { runCatching { repo.saveProgress(p) } }
    }

    override fun onCleared() {
        saveProgress()
        narratorApi.stop()
        super.onCleared()
    }

    companion object {
        private const val SAVE_EVERY_MS = 3_000L
    }
}
