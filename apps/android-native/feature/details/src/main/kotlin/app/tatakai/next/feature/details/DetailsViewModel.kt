package app.tatakai.next.feature.details

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.Episode
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.StreamSource
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Bottom/side sheet that lets the user pick a source for an episode (and optionally enhance it on the paired PC). */
data class SourceSheet(
    val episode: Int,
    val sources: LoadState<List<StreamSource>> = LoadState.Loading,
    val startingRemote: Boolean = false,
    val remoteError: AppError? = null,
)

data class DetailsUiState(
    val media: LoadState<Media> = LoadState.Loading,
    val episodes: LoadState<List<Episode>> = LoadState.Loading,
    val progress: Map<Int, HistoryEntry> = emptyMap(),
    val sheet: SourceSheet? = null,
    /** A reachable paired PC exists, so "Enhance on PC" is offered. */
    val pcAvailable: Boolean = false,
) {
    /** Episode the primary button resumes / starts. */
    val continueEpisode: Int
        get() = continueTarget(progress, (media as? LoadState.Ok)?.value?.episodes)
}

sealed interface DetailsEvent {
    data class NavigateToPlayer(val args: PlayerArgs) : DetailsEvent

    /** No source for an episode: send the user to install/enable an extension. */
    data object OpenExtensions : DetailsEvent
}

/** Last watched episode if unfinished, else the one after it (bounded by [total]); 1 when nothing was watched. */
fun continueTarget(progress: Map<Int, HistoryEntry>, total: Int?): Int {
    val last = progress.values.maxByOrNull { it.watchedAt } ?: return 1
    if (!last.completed) return last.episode
    val next = last.episode + 1
    return if (total != null && next > total) last.episode else next
}

class DetailsViewModel(
    private val kind: MediaKind,
    private val ref: MediaRef,
    private val metadata: MetadataRepository,
    history: HistoryRepository,
    private val extensions: ExtensionRepository,
    private val link: LinkRepository,
) : ViewModel() {

    private val _state = MutableStateFlow(DetailsUiState())
    val state: StateFlow<DetailsUiState> = _state.asStateFlow()

    private val _events = Channel<DetailsEvent>(Channel.BUFFERED)
    val events: Flow<DetailsEvent> = _events.receiveAsFlow()

    init {
        load()
        ref.anilistId?.let { id ->
            viewModelScope.launch {
                history.progressFor(id).catch { }.collect { map -> _state.update { it.copy(progress = map) } }
            }
        }
        viewModelScope.launch {
            link.state.collect { s ->
                _state.update { it.copy(pcAvailable = s is LinkState.Paired && s.reachable) }
            }
        }
    }

    fun retry() {
        _state.update { it.copy(media = LoadState.Loading, episodes = LoadState.Loading) }
        load()
    }

    private fun load() {
        viewModelScope.launch {
            val result = runCatchingApp { metadata.media(kind, ref) }
            _state.update {
                it.copy(
                    media = result.fold(
                        onSuccess = { m -> if (m == null) LoadState.Err(AppError.Http(404)) else LoadState.Ok(m) },
                        onFailure = { e -> LoadState.Err(e.toAppError()) },
                    ),
                )
            }
        }
        viewModelScope.launch {
            val result = runCatchingApp { metadata.episodes(ref).sortedBy { it.number } }
            _state.update {
                it.copy(
                    episodes = result.fold(
                        onSuccess = { eps -> LoadState.Ok(eps) },
                        onFailure = { e -> LoadState.Err(e.toAppError()) },
                    ),
                )
            }
        }
    }

    /** Opens the source picker for [episode] and resolves its streams through the installed extensions. */
    fun onEpisodeClick(episode: Int) {
        val media = (_state.value.media as? LoadState.Ok)?.value ?: return
        _state.update { it.copy(sheet = SourceSheet(episode)) }
        viewModelScope.launch {
            val result = runCatchingApp { extensions.resolveStreams(media, episode) }
            _state.update { s ->
                val sheet = s.sheet?.takeIf { it.episode == episode } ?: return@update s
                s.copy(
                    sheet = sheet.copy(
                        sources = result.fold(
                            onSuccess = { LoadState.Ok(it) },
                            onFailure = { e -> LoadState.Err(e.toAppError()) },
                        ),
                    ),
                )
            }
        }
    }

    fun onContinueClick() = onEpisodeClick(_state.value.continueEpisode)

    fun dismissSheet() = _state.update { it.copy(sheet = null) }

    /** The player resolves and falls back between sources on its own, so any picked row simply starts playback. */
    fun onPlay() {
        val sheet = _state.value.sheet ?: return
        emitPlayer(sheet.episode, remoteSessionId = null)
        dismissSheet()
    }

    fun onPlayOnPc(source: StreamSource) {
        val sheet = _state.value.sheet ?: return
        if (sheet.startingRemote) return
        _state.update { it.copy(sheet = sheet.copy(startingRemote = true, remoteError = null)) }
        viewModelScope.launch {
            val result = runCatchingApp {
                link.startSession(LinkSource.Url(source.url, source.headers), profileId = null).getOrThrow()
            }
            result.onSuccess { session ->
                emitPlayer(sheet.episode, remoteSessionId = session.sessionId)
                dismissSheet()
            }.onFailure { e ->
                _state.update { s ->
                    s.copy(sheet = s.sheet?.copy(startingRemote = false, remoteError = e.toAppError()))
                }
            }
        }
    }

    fun onOpenExtensions() {
        dismissSheet()
        _events.trySend(DetailsEvent.OpenExtensions)
    }

    private fun emitPlayer(episode: Int, remoteSessionId: String?) {
        val s = _state.value
        val title = (s.media as? LoadState.Ok)?.value?.title ?: return
        val saved = s.progress[episode]
        val startMs = if (saved != null && !saved.completed) saved.progressSec * 1000L else 0L
        _events.trySend(
            DetailsEvent.NavigateToPlayer(
                PlayerArgs(media = ref, title = title, episode = episode, startMs = startMs, remoteSessionId = remoteSessionId),
            ),
        )
    }
}
