package app.tatakai.next.feature.details

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.Episode
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedKind
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
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrentUri
import app.tatakai.next.ui.media.distinctByKey
import app.tatakai.next.ui.state.runCatchingApp
import app.tatakai.next.ui.state.toAppError
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.catch
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** Source summary panel of Details (sources.js `summary`): the ranked candidates of [episode] (the episode Watch / Continue plays). */
data class SourcePanel(
    val episode: Int,
    val sources: LoadState<List<RankedSource>> = LoadState.Loading,
)

/** The source picker is open for [episode]; [playOnPick] = opened from the split button ▾ (picking starts playback). */
data class PickerRequest(val episode: Int, val playOnPick: Boolean)

data class DetailsUiState(
    val media: LoadState<Media> = LoadState.Loading,
    val episodes: LoadState<List<Episode>> = LoadState.Loading,
    val progress: Map<Int, HistoryEntry> = emptyMap(),
    /** null until the media loaded. */
    val panel: SourcePanel? = null,
    /** The user's manual pick ([RankedSource.key]) for [panel]'s episode; null = the system's choice (the recommended row). */
    val selectedKey: String? = null,
    val mode: SourceMode = SourceMode.AUTO,
    /** At least one enabled extension: otherwise the panel shows the "no source installed" empty state. */
    val hasExtensions: Boolean = true,
    val picker: PickerRequest? = null,
    val link: LinkState = LinkState.Unpaired,
    /** A Link session is being started for "Enviar a dispositivo". */
    val sending: Boolean = false,
    /** Related tab content; null = not requested yet (loaded the first time the tab opens). */
    val related: LoadState<List<Media>>? = null,
) {
    /** Episode the primary button resumes / starts. */
    val continueEpisode: Int
        get() = continueTarget(progress, (media as? LoadState.Ok)?.value?.episodes)

    /** The history entry of [continueEpisode] when it is unfinished ("Retomar en 12:34"). */
    val resumeEntry: HistoryEntry?
        get() = progress[continueEpisode]?.takeIf { !it.completed && it.progressSec > 0 }

    /** A reachable paired PC exists. */
    val pcAvailable: Boolean get() = (link as? LinkState.Paired)?.reachable == true

    val pairedPc: PairedPc? get() = (link as? LinkState.Paired)?.pc

    val panelSources: List<RankedSource> get() = (panel?.sources as? LoadState.Ok)?.value.orEmpty()

    /** The row the panel shows: the user's pick, else the recommended (first) one. */
    val shownSource: RankedSource? get() = panelSources.firstOrNull { it.key == selectedKey } ?: panelSources.firstOrNull()

    /** Best torrent of the panel episode (ranked order, extension enabled): what "Descargar" uses. */
    val bestTorrent: RankedSource? get() = panelSources.bestTorrent()

    /** The best torrent is a season pack / batch: "Descargar temporada" is offered (11 §3.4). */
    val canDownloadSeason: Boolean get() = bestTorrent?.candidate?.tags?.batch == true
}

internal fun List<RankedSource>.bestTorrent(): RankedSource? =
    firstOrNull { it.candidate.kind == SourceKind.TORRENT && it.extEnabled && it.candidate.torrentUri() != null }

sealed interface DetailsEvent {
    data class NavigateToPlayer(val args: PlayerArgs) : DetailsEvent

    /** The panel's source changed through the picker (snackbar "Se usará X"). */
    data class SourceChosen(val extName: String, val label: String) : DetailsEvent

    data class DownloadQueued(val torrentId: String) : DetailsEvent

    data class DownloadFailed(val cause: AppError) : DetailsEvent

    /** "Descargar" without any usable torrent candidate. */
    data object NoTorrentSource : DetailsEvent

    data class SendFailed(val cause: AppError) : DetailsEvent

    /** "Enviar a dispositivo" needs a web stream (the PC fetches a URL). */
    data object NoWebSource : DetailsEvent
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
    settings: SettingsRepository? = null,
    private val torrents: TorrentRepository? = null,
    private val stats: SourceStatsStore? = null,
) : ViewModel() {

    private val _state = MutableStateFlow(DetailsUiState())
    val state: StateFlow<DetailsUiState> = _state.asStateFlow()

    private val _events = Channel<DetailsEvent>(Channel.BUFFERED)
    val events: Flow<DetailsEvent> = _events.receiveAsFlow()

    private var panelJob: Job? = null

    private val mediaOrNull: Media? get() = (_state.value.media as? LoadState.Ok)?.value

    init {
        // Progress first: when it is already cached, the first panel resolution targets the right episode (no double resolve).
        ref.anilistId?.let { id ->
            viewModelScope.launch {
                history.progressFor(id).catch { }.collect { map ->
                    _state.update { it.copy(progress = map) }
                    ensurePanel()
                }
            }
        }
        load()
        viewModelScope.launch {
            link.state.collect { s -> _state.update { it.copy(link = s) } }
        }
        if (settings != null) {
            viewModelScope.launch {
                settings.settings.collect { s -> _state.update { it.copy(mode = s.sourcePrefs.mode) } }
            }
        }
        viewModelScope.launch {
            // docs/redesign/13 §3.10: a linked PC extension server also counts as a source.
            combine(extensions.installed, extensions.externalSourcesAvailable) { list, external -> list.any { e -> e.enabled } || external }
                .catch { }
                .collect { has -> _state.update { it.copy(hasExtensions = has) } }
        }
    }

    fun retry() {
        _state.update { it.copy(media = LoadState.Loading, episodes = LoadState.Loading, panel = null) }
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
            ensurePanel()
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

    // ---- source panel ----

    /** Resolves the candidates of the episode Watch / Continue plays, once per target episode. */
    private fun ensurePanel() {
        if (mediaOrNull == null) return
        val target = _state.value.continueEpisode
        if (_state.value.panel?.episode == target) return
        resolveSources(target)
    }

    private fun resolveSources(episode: Int) {
        val media = mediaOrNull ?: return
        panelJob?.cancel()
        _state.update { it.copy(panel = SourcePanel(episode), selectedKey = null) }
        panelJob = viewModelScope.launch {
            val result = runCatchingApp { extensions.resolveCandidates(media, episode) }
            _state.update { s ->
                if (s.panel?.episode != episode) return@update s
                val sources = result.getOrNull()
                s.copy(
                    panel = SourcePanel(
                        episode = episode,
                        sources = result.fold(
                            onSuccess = { LoadState.Ok(it) },
                            onFailure = { e -> LoadState.Err(e.toAppError()) },
                        ),
                    ),
                    selectedKey = sources?.firstOrNull { it.picked }?.key,
                )
            }
        }
    }

    /** Retry of the panel / picker error. */
    fun retrySources() {
        val episode = _state.value.panel?.episode ?: _state.value.continueEpisode
        resolveSources(episode)
    }

    /** ▾ of the split button ([playOnPick] = true) or "Ver todas las fuentes (n)" of the panel (false). */
    fun openPicker(playOnPick: Boolean) {
        val panel = _state.value.panel ?: return
        _state.update { it.copy(picker = PickerRequest(panel.episode, playOnPick)) }
    }

    fun dismissPicker() = _state.update { it.copy(picker = null) }

    fun onPick(source: RankedSource) {
        val request = _state.value.picker ?: return
        _state.update { it.copy(picker = null, selectedKey = source.key) }
        val mediaId = HistoryEntry.animeIdOf(ref.anilistId, ref.malId)
        val store = stats
        if (mediaId != null && store != null) {
            viewModelScope.launch { runCatchingApp { store.setPickedSource(mediaId, request.episode, source.key) } }
        }
        if (request.playOnPick) {
            emitPlayer(request.episode, sourceKey = source.key)
        } else {
            _events.trySend(DetailsEvent.SourceChosen(source.candidate.extName, source.candidate.label))
        }
    }

    // ---- playback ----

    /** Leading half of the split button / floating toolbar: Watch or Continue with the panel's choice. */
    fun onWatch() {
        val episode = _state.value.continueEpisode
        emitPlayer(episode, sourceKey = sourceKeyFor(episode))
    }

    /** An episode row: straight to the player (the player resolves and fails over; Manual mode asks there). */
    fun onEpisodeClick(episode: Int) = emitPlayer(episode, sourceKey = sourceKeyFor(episode))

    private fun sourceKeyFor(episode: Int): String? =
        _state.value.selectedKey?.takeIf { _state.value.panel?.episode == episode }

    // ---- downloads (the composable wraps every call in rememberDownloadGate().launch { ... }) ----

    /** "Descargar" on a torrent row of the picker / panel; [episode] null = whole torrent (season pack). */
    fun download(source: RankedSource, episode: Int?) {
        val media = mediaOrNull ?: return
        val uri = source.candidate.torrentUri()
        if (uri == null) {
            _events.trySend(DetailsEvent.NoTorrentSource)
            return
        }
        val repo = torrents
        if (repo == null) {
            _events.trySend(DetailsEvent.DownloadFailed(AppError.Unknown("torrent engine unavailable")))
            return
        }
        viewModelScope.launch {
            val result = runCatchingApp { repo.addDownload(uri, media, episode).getOrThrow() }
            _events.trySend(
                result.fold(
                    onSuccess = { id -> DetailsEvent.DownloadQueued(id) },
                    onFailure = { e -> DetailsEvent.DownloadFailed(e.toAppError()) },
                ),
            )
        }
    }

    /** "Descargar episodio" of an episode row: the best torrent of that episode (resolved on demand when it is not the panel's). */
    fun downloadEpisode(episode: Int) {
        val media = mediaOrNull ?: return
        val s = _state.value
        val panel = s.panel
        if (panel != null && panel.episode == episode && panel.sources is LoadState.Ok) {
            val best = s.bestTorrent
            if (best == null) _events.trySend(DetailsEvent.NoTorrentSource) else download(best, episode)
            return
        }
        viewModelScope.launch {
            runCatchingApp { extensions.resolveCandidates(media, episode) }
                .onSuccess { list ->
                    val best = list.bestTorrent()
                    if (best == null) _events.trySend(DetailsEvent.NoTorrentSource) else download(best, episode)
                }
                .onFailure { e -> _events.trySend(DetailsEvent.DownloadFailed(e.toAppError())) }
        }
    }

    /** "Descargar temporada": the best torrent is a batch, download it whole. */
    fun downloadSeason() {
        val best = _state.value.bestTorrent?.takeIf { it.candidate.tags.batch }
        if (best == null) _events.trySend(DetailsEvent.NoTorrentSource) else download(best, episode = null)
    }

    // ---- send to device (Tatakai Link) ----

    /** Starts a Link session on the paired PC with a web stream of [DetailsUiState.continueEpisode] and opens the player on it. */
    fun sendToDevice() {
        val media = mediaOrNull ?: return
        if (_state.value.sending) return
        val episode = _state.value.continueEpisode
        _state.update { it.copy(sending = true) }
        viewModelScope.launch {
            val stream = runCatchingApp { webStreamFor(media, episode) }.getOrNull()
            if (stream == null) {
                _state.update { it.copy(sending = false) }
                _events.trySend(DetailsEvent.NoWebSource)
                return@launch
            }
            val result = runCatchingApp {
                link.startSession(LinkSource.Url(stream.url, stream.headers), profileId = null).getOrThrow()
            }
            _state.update { it.copy(sending = false) }
            result.onSuccess { session -> emitPlayer(episode, sourceKey = null, remoteSessionId = session.sessionId) }
                .onFailure { e -> _events.trySend(DetailsEvent.SendFailed(e.toAppError())) }
        }
    }

    private suspend fun webStreamFor(media: Media, episode: Int): StreamSource? {
        val s = _state.value
        val panel = s.panel
        val candidates = if (panel != null && panel.episode == episode && panel.sources is LoadState.Ok) {
            s.panelSources
        } else {
            runCatchingApp { extensions.resolveCandidates(media, episode) }.getOrDefault(emptyList())
        }
        val preferred = candidates.firstOrNull { it.key == s.selectedKey && it.candidate.kind == SourceKind.WEB }
        val fromCandidates = (listOfNotNull(preferred) + candidates)
            .asSequence()
            .filter { it.candidate.kind == SourceKind.WEB && it.extEnabled }
            .mapNotNull { runCatching { it.candidate.toStream() }.getOrNull() }
            .firstOrNull()
        return fromCandidates ?: extensions.resolveStreams(media, episode).firstOrNull()
    }

    // ---- related ----

    /**
     * Related tab: the metadata API has no relations endpoint, so this lists titles of the Trending + Popular feeds that share a
     * genre with this one (prototype: same-genre titles). Loaded once, on first open.
     */
    fun loadRelated() {
        val media = mediaOrNull ?: return
        val current = _state.value.related
        if (current != null && current !is LoadState.Err) return
        _state.update { it.copy(related = LoadState.Loading) }
        viewModelScope.launch {
            val result = runCatchingApp {
                RELATED_FEEDS.flatMap { metadata.feed(MediaKind.ANIME, it).items }
                    .distinctByKey()
                    .filter { it.ref != media.ref && it.genres.any { g -> g in media.genres } }
                    .take(RELATED_LIMIT)
            }
            _state.update {
                it.copy(
                    related = result.fold(
                        onSuccess = { list -> LoadState.Ok(list) },
                        onFailure = { e -> LoadState.Err(e.toAppError()) },
                    ),
                )
            }
        }
    }

    private fun emitPlayer(episode: Int, sourceKey: String?, remoteSessionId: String? = null) {
        val s = _state.value
        val title = (s.media as? LoadState.Ok)?.value?.title ?: return
        val saved = s.progress[episode]
        val startMs = if (saved != null && !saved.completed) saved.progressSec * 1000L else 0L
        _events.trySend(
            DetailsEvent.NavigateToPlayer(
                PlayerArgs(
                    media = ref,
                    title = title,
                    episode = episode,
                    startMs = startMs,
                    remoteSessionId = remoteSessionId,
                    sourceKey = sourceKey,
                ),
            ),
        )
    }

    private companion object {
        val RELATED_FEEDS = listOf(FeedKind.TRENDING, FeedKind.POPULAR)
        const val RELATED_LIMIT = 12
    }
}
