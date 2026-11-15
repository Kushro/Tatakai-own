package app.tatakai.next.feature.details

import app.tatakai.next.model.RelationsView
import app.tatakai.next.feature.player.sources.rankForThisDevice
import app.tatakai.next.link.LinkApiError
import app.tatakai.next.link.LinkError
import app.tatakai.next.link.linkErrorOrNull
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import app.tatakai.next.model.AppError
import app.tatakai.next.model.Episode
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.CharacterEdge
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProgress
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaDetails
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.Recommendation
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.StaffEdge
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrentUri
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
import kotlinx.coroutines.withTimeoutOrNull

/** Source summary panel of Details (sources.js `summary`): the ranked candidates of [episode] (the episode Watch / Continue plays). */
data class SourcePanel(
    val episode: Int,
    val sources: LoadState<List<RankedSource>> = LoadState.Loading,
    /** BUG-011 / BUG-019: [sources] is a partial list and more extensions / servers are still answering. */
    val loadingMore: Boolean = false,
    /** BUG-140: the remembered pick for [episode] ([RankedSource.key], or the one chosen here); the picker says when it is missing. */
    val rememberedPick: String? = null,
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
    // --- parity wave A (15 §5 o.2, worker o) ---
    /** Technical sheet, trailer, next airing, studios (loaded with the media); Ok(null) = the provider has nothing. */
    val details: LoadState<MediaDetails?> = LoadState.Loading,
    /** Related tab: AniList relations (real edges, each opens its own Details); null = not requested yet. */
    val relations: LoadState<List<MediaRelation>>? = null,
    /** Seasons / watch order (PREQUEL / SEQUEL chain), loaded with the Related tab. */
    val seasons: LoadState<List<Media>>? = null,
    val recommendations: LoadState<List<Recommendation>>? = null,
    /** Characters tab (paged) + staff. */
    val characters: CharactersState? = null,
    val staff: LoadState<List<StaffEdge>>? = null,
    val titleLanguage: TitleLanguage = TitleLanguage.ROMAJI,
    /** 18+ title not confirmed yet for this id (classic AdultWarningGate): the screen shows the gate instead of the content. */
    val adultGate: Boolean = false,
    /** AppSettings.matureContent: SHOW / BLUR / WARN (see [adultPresentation]). */
    val matureContent: MatureContentMode = MatureContentMode.WARN,
    /** BLUR mode: banner and cover of this 18+ title stay blurred until tapped once (not remembered). */
    val coverBlurred: Boolean = false,
    val episodeQuery: String = "",
    /** parity wave B (r, downloads.local-library.play-offline): episodes with a completed web download (marked, played offline). */
    val downloaded: Set<Int> = emptySet(),
    val episodeView: EpisodeView = EpisodeView.LIST,
    /** 2026-11 UI refresh: Relacionados as a graph or a grouped list. */
    val relationsView: RelationsView = RelationsView.GRAPH,
    /** The title's row of the local list mirror (AniList / MAL); null = not on the list (or no lists layer). */
    val listEntry: ListEntry? = null,
    /** BUG-039: "Volver a comprobar" of the send dialog is asking the PC. */
    val pcChecking: Boolean = false,
) {
    private val total: Int? get() = listEntry?.totalEpisodes?.takeIf { it > 0 } ?: (media as? LoadState.Ok)?.value?.episodes

    /** Episodes watched according to the list (P); 0 when not on the list. */
    val listProgress: Int get() = listEntry?.progress ?: 0

    /** Today's history rule alone (last unfinished episode, else the next one). */
    private val historyTarget: Int get() = continueTarget(progress, total)

    /** Episode the primary button resumes / starts: max(history, list progress + 1), capped to the total (ListProgress.resumeEpisode). */
    val continueEpisode: Int
        get() = ListProgress.resumeEpisode(historyTarget, listProgress, total)

    /** Something was watched (history or list): "Continuar" instead of "Ver". */
    val hasProgress: Boolean get() = progress.isNotEmpty() || listProgress > 0

    /** The history entry of [continueEpisode] when it is unfinished ("Retomar en 12:34"); only when the history episode is the one shown. */
    val resumeEntry: HistoryEntry?
        get() = progress[continueEpisode]?.takeIf { continueEpisode == historyTarget && !it.completed && it.progressSec > 0 }

    /** Episode row mark: within the list progress, or its history row is completed. */
    fun isWatched(episode: Int): Boolean = ListProgress.isEpisodeWatched(episode, listProgress, progress[episode])

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

    val detailsOrNull: MediaDetails? get() = (details as? LoadState.Ok)?.value
}

/** Characters tab: the edges loaded so far; [error] = the last page failed (the list stays, a retry row shows). */
data class CharactersState(
    val items: List<CharacterEdge> = emptyList(),
    val page: Int = 0,
    val hasNext: Boolean = true,
    val loading: Boolean = false,
    val error: AppError? = null,
)

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

    data class SendFailed(val cause: AppError, val reason: SendFailReason = SendFailReason.OTHER) : DetailsEvent

    /** BUG-009/023: the session started with [label] instead of the chosen source ([chosenWasTorrent]: it was a torrent). */
    data class SentOther(val label: String, val chosenWasTorrent: Boolean) : DetailsEvent

    /** "Enviar a dispositivo" needs a web stream (the PC fetches a URL). */
    data object NoWebSource : DetailsEvent

    /** "Marcar como visto / no visto" could not update the list. */
    data class ListUpdateFailed(val cause: AppError) : DetailsEvent
}

/** Resolving the web sources for "Enviar a dispositivo" gives up after this long (BUG-023: no more silent minutes). */
internal const val SEND_RESOLVE_TIMEOUT_MS = 60_000L

/** Web sources tried in turn when the PC refuses or cannot open one. */
internal const val MAX_SEND_ATTEMPTS = 3

/** Why "Enviar a dispositivo" failed, for a message without technical codes (BUG-023). */
enum class SendFailReason(val sourceSpecific: Boolean) {
    /** The PC's policy refused the source URL (`url_source_denied`). */
    SOURCE_DENIED(true),

    /** The PC could not read the source (`source_unavailable`). */
    SOURCE_UNAVAILABLE(true),

    /** The chosen enhancement profile is not available on the PC (`profile_unavailable`). */
    PROFILE_UNAVAILABLE(false),

    /** The PC did not answer. */
    OFFLINE(false),

    /** This phone is no longer paired / was revoked. */
    NOT_PAIRED(false),

    /** Resolving the sources took longer than [SEND_RESOLVE_TIMEOUT_MS]. */
    RESOLVE_TIMEOUT(false),
    OTHER(false),
}

/** [SendFailReason] of a failed `startSession`. Pure. */
fun sendFailReason(e: Throwable): SendFailReason = when (val le = e.linkErrorOrNull()) {
    is LinkError.Api -> when (le.error) {
        LinkApiError.UrlSourceDenied -> SendFailReason.SOURCE_DENIED
        LinkApiError.SourceUnavailable, LinkApiError.ExtensionUnavailable -> SendFailReason.SOURCE_UNAVAILABLE
        LinkApiError.ProfileUnavailable -> SendFailReason.PROFILE_UNAVAILABLE
        LinkApiError.DeviceNotAuthorized, LinkApiError.ClientCertificateRequired -> SendFailReason.NOT_PAIRED
        else -> SendFailReason.OTHER
    }
    is LinkError.Transport.Refused, is LinkError.Transport.Timeout -> SendFailReason.OFFLINE
    is LinkError.Transport.Tls.Revoked, is LinkError.Transport.Tls.Expired, is LinkError.NotPaired -> SendFailReason.NOT_PAIRED
    else -> if (e.toAppError() == AppError.Offline) SendFailReason.OFFLINE else SendFailReason.OTHER
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
    private val history: HistoryRepository,
    private val extensions: ExtensionRepository,
    private val link: LinkRepository,
    private val settings: SettingsRepository? = null,
    private val torrents: TorrentRepository? = null,
    private val stats: SourceStatsStore? = null,
    private val prefs: DetailsPrefs = InMemoryDetailsPrefs(),
    // parity wave B (r): completed downloads play without network; null when :core:download is not bound.
    private val downloads: DownloadRepository? = null,
    /** The local list mirror (dataModule); null = no lists layer: no list progress and no manual watched toggle. */
    private val lists: ListsRepository? = null,
) : ViewModel() {

    // 2026-11 UI refresh: the episode / relations views come from Diseño y estilo (the private pref only when no settings are bound).
    private val _state = MutableStateFlow(
        DetailsUiState(
            episodeView = settings?.settings?.value?.look?.episodeView ?: prefs.episodeView(),
            relationsView = settings?.settings?.value?.look?.relationsView ?: RelationsView.GRAPH,
        ),
    )
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
        lists?.let { repo ->
            viewModelScope.launch {
                repo.entry(ref).catch { }.collect { e ->
                    _state.update { it.copy(listEntry = e) }
                    ensurePanel()
                }
            }
        }
        load()
        downloads?.let { repo ->
            viewModelScope.launch {
                repo.downloadedEpisodes(ref).catch { }.collect { map -> _state.update { it.copy(downloaded = map.keys) } }
            }
        }
        viewModelScope.launch {
            link.state.collect { s -> _state.update { it.copy(link = s) } }
        }
        if (settings != null) {
            viewModelScope.launch {
                settings.settings.collect { s ->
                    _state.update {
                        it.copy(
                            mode = s.sourcePrefs.mode,
                            titleLanguage = s.titleLanguage,
                            matureContent = s.matureContent,
                            episodeView = s.look.episodeView,
                            relationsView = s.look.relationsView,
                        )
                    }
                    updateAdultGate()
                }
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
        _state.update { it.copy(media = LoadState.Loading, episodes = LoadState.Loading, details = LoadState.Loading, panel = null) }
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
            updateAdultGate()
        }
        loadDetails()
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
            // BUG-011 / BUG-019: the panel (and its picker) fills in as extensions and linked servers answer.
            var last: List<RankedSource>? = null
            fun publish(sources: LoadState<List<RankedSource>>, more: Boolean) = _state.update { s ->
                if (s.panel?.episode != episode) return@update s
                val list = (sources as? LoadState.Ok)?.value
                s.copy(
                    panel = s.panel.copy(sources = sources, loadingMore = more),
                    // The user's choice in the picker survives the updates; otherwise the remembered pick.
                    selectedKey = s.selectedKey?.takeIf { k -> list?.any { it.key == k } == true } ?: list?.firstOrNull { it.picked }?.key,
                )
            }
            val mediaId = HistoryEntry.animeIdOf(ref.anilistId, ref.malId)
            val remembered = mediaId?.let { id -> stats?.let { store -> runCatchingApp { store.pickedSource(id, episode) }.getOrNull() } }
            if (remembered != null) {
                _state.update { s ->
                    val p = s.panel?.takeIf { it.episode == episode && it.rememberedPick == null } ?: return@update s
                    s.copy(panel = p.copy(rememberedPick = remembered))
                }
            }
            val result = runCatchingApp {
                extensions.candidatesFlow(media, episode).collect { partial ->
                    val ranked = rankForThisDevice(partial)
                    last = ranked
                    if (ranked.isNotEmpty()) publish(LoadState.Ok(ranked), more = true)
                }
            }
            publish(
                result.fold(onSuccess = { LoadState.Ok(last.orEmpty()) }, onFailure = { e -> last?.let { LoadState.Ok(it) } ?: LoadState.Err(e.toAppError()) }),
                more = false,
            )
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
        _state.update { s ->
            val panel = s.panel?.let { p -> if (p.episode == request.episode) p.copy(rememberedPick = source.key) else p }
            s.copy(picker = null, selectedKey = source.key, panel = panel)
        }
        chosenInPanel = request.episode to source.key
        val mediaId = HistoryEntry.animeIdOf(ref.anilistId, ref.malId)
        val store = stats
        if (mediaId != null && store != null) {
            viewModelScope.launch { runCatchingApp { store.setPickedSource(mediaId, request.episode, source.key) } }
        }
        if (request.playOnPick) {
            emitPlayer(request.episode, sourceKey = source.key, preferLocal = false, sourceConfirmed = true)
        } else {
            _events.trySend(DetailsEvent.SourceChosen(source.candidate.extName, source.candidate.label))
        }
    }

    // ---- playback ----

    /** Leading half of the split button / floating toolbar: Watch or Continue with the panel's choice. */
    fun onWatch() {
        val s = _state.value
        val episode = s.continueEpisode
        // the saved position only when the history episode is the one shown (the list progress may point past it)
        emitPlayer(episode, sourceKey = sourceKeyFor(episode), resume = s.resumeEntry != null)
    }

    /** True when the manual watched toggle is available (a lists layer is bound). */
    val canToggleWatched: Boolean get() = lists != null

    /**
     * Episode row toggle (ListProgress): "Marcar como visto" sets P = max(P, n) (adds the title as WATCHING when it is not on the
     * list; reaching the total completes it, like the automatic progress); "Marcar como no visto" sets P = min(P, n - 1) and clears
     * the completed flag of the local history rows >= n. Goes through [ListsRepository] (local first, queued for linked accounts).
     */
    fun toggleWatched(episode: Int) {
        val repo = lists ?: return
        val media = mediaOrNull ?: return
        val s = _state.value
        val entry = s.listEntry
        val watched = s.isWatched(episode)
        val total = entry?.totalEpisodes?.takeIf { it > 0 } ?: media.episodes
        viewModelScope.launch {
            val result = runCatchingApp {
                if (watched) {
                    if (entry != null && entry.progress >= episode) {
                        // a finished title becomes Viendo again (P = n - 1 < T)
                        ListProgress.statusAfterUnmark(entry.status)?.let { repo.setStatus(media, it).getOrThrow() }
                        repo.setProgress(media.ref, ListProgress.toggledProgress(entry.progress, episode, watched = false)).getOrThrow()
                    }
                    history.clearCompleted(media.ref, episode)
                } else {
                    if (entry == null) repo.setStatus(media, ListStatus.WATCHING).getOrThrow()
                    repo.setProgress(media.ref, ListProgress.toggledProgress(entry?.progress ?: 0, episode, watched = true, total = total)).getOrThrow()
                }
            }
            result.onFailure { e -> _events.trySend(DetailsEvent.ListUpdateFailed(e.toAppError())) }
        }
    }

    /** An episode row: straight to the player (the player resolves and fails over; Manual mode asks there). */
    fun onEpisodeClick(episode: Int) = emitPlayer(episode, sourceKey = sourceKeyFor(episode))

    /**
     * A source the user chose in this screen's picker (not yet played). The panel's default (the remembered pick, loaded once) is not
     * passed on: the player reads the remembered pick itself, so a source changed inside the player is not overridden by a stale panel
     * (BUG-018: A-11 went back to Latino after picking anizone in the player). One-shot: cleared once the player opens.
     */
    private var chosenInPanel: Pair<Int, String>? = null

    private fun sourceKeyFor(episode: Int): String? =
        chosenInPanel?.takeIf { it.first == episode && _state.value.selectedKey == it.second }?.second

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

    /**
     * Starts a Link session on the paired PC with a web stream of [DetailsUiState.continueEpisode] and opens the player on it.
     * BUG-023: [DetailsUiState.sending] drives a visible "Enviando…" state; resolving is bounded by [SEND_RESOLVE_TIMEOUT_MS];
     * when the PC refuses or cannot open one web source the next ones are tried ([MAX_SEND_ATTEMPTS]); a failure always ends in
     * [DetailsEvent.SendFailed] with a [SendFailReason], and a source other than the chosen one is announced ([DetailsEvent.SentOther]).
     */
    /**
     * BUG-039: the send dialog with the PC "No responde" offers to check again instead of a send that would fail. Any Link call
     * refreshes the reachability ([LinkState.Paired.reachable]); the dialog turns into "Enviar" when the PC answers.
     */
    fun recheckPc() {
        if (_state.value.pcChecking) return
        _state.update { it.copy(pcChecking = true) }
        viewModelScope.launch {
            runCatching { link.capabilities() }
            _state.update { it.copy(pcChecking = false) }
        }
    }

    fun sendToDevice() {
        val media = mediaOrNull ?: return
        if (_state.value.sending) return
        val episode = _state.value.continueEpisode
        _state.update { it.copy(sending = true) }
        viewModelScope.launch {
            try {
                val resolved = withTimeoutOrNull(SEND_RESOLVE_TIMEOUT_MS) { runCatchingApp { webStreamsFor(media, episode) } }
                if (resolved == null) {
                    _events.trySend(DetailsEvent.SendFailed(AppError.Unknown("timeout"), SendFailReason.RESOLVE_TIMEOUT))
                    return@launch
                }
                val streams = resolved.getOrDefault(emptyList())
                if (streams.isEmpty()) {
                    _events.trySend(DetailsEvent.NoWebSource)
                    return@launch
                }
                var lastError: Throwable? = null
                for ((index, sendable) in streams.take(MAX_SEND_ATTEMPTS).withIndex()) {
                    val result = runCatchingApp { link.startSession(sendable.source, profileId = null).getOrThrow() }
                    val session = result.getOrNull()
                    if (session != null) {
                        if (index > 0 || !sendable.chosen) {
                            val chosen = _state.value.selectedKey?.let { k -> _state.value.panelSources.firstOrNull { it.key == k } }
                            if (chosen != null && chosen.key != sendable.key) {
                                _events.trySend(DetailsEvent.SentOther(sendable.label, chosen.candidate.kind == SourceKind.TORRENT))
                            }
                        }
                        emitPlayer(episode, sourceKey = null, remoteSessionId = session.sessionId, preferLocal = false)
                        return@launch
                    }
                    val e = result.exceptionOrNull()
                    lastError = e
                    // a torrent the PC cannot open (older PC, no peers, no file) falls back to the web sources
                    if (e == null || !(sendable.source is LinkSource.Torrent || sendFailReason(e).sourceSpecific)) break
                }
                val e = lastError ?: return@launch
                _events.trySend(DetailsEvent.SendFailed(e.toAppError(), sendFailReason(e)))
            } finally {
                _state.update { it.copy(sending = false) }
            }
        }
    }

    /** One source "Enviar a dispositivo" can offer the PC, in order of preference (BUG-009: a torrent the PC streams itself). */
    private data class Sendable(val source: LinkSource, val key: String?, val label: String, val chosen: Boolean)

    private suspend fun webStreamsFor(media: Media, episode: Int): List<Sendable> {
        val s = _state.value
        val panel = s.panel
        val candidates = if (panel != null && panel.episode == episode && panel.sources is LoadState.Ok) {
            s.panelSources
        } else {
            runCatchingApp { extensions.resolveCandidates(media, episode) }.getOrDefault(emptyList())
        }
        val torrent = sendableTorrent(candidates, s.selectedKey, media.ref.anilistId, episode)
        val preferred = candidates.firstOrNull { it.key == s.selectedKey && it.candidate.kind == SourceKind.WEB }
        val webs = (listOfNotNull(preferred) + candidates)
            .distinctBy { it.key }
            .filter { it.candidate.kind == SourceKind.WEB && it.extEnabled }
            .mapNotNull { r ->
                runCatching { r.candidate.toStream() }.getOrNull()?.let {
                    Sendable(LinkSource.Url(it.url, it.headers), r.key, "${r.candidate.extName} · ${r.candidate.label}", chosen = r.key == s.selectedKey)
                }
            }
            .ifEmpty { extensions.resolveStreams(media, episode).map { Sendable(LinkSource.Url(it.url, it.headers), key = null, label = it.provider, chosen = false) } }
        return listOfNotNull(torrent) + webs
    }

    /**
     * BUG-009: the torrent to send first: the chosen source when it is a torrent, else the top-ranked one when it is a torrent
     * (the order the player uses). Needs the AniList id (the PC's title id). Null = web sources only.
     */
    private fun sendableTorrent(candidates: List<RankedSource>, selectedKey: String?, anilistId: Int?, episode: Int): Sendable? {
        val id = anilistId ?: return null
        val pick = candidates.firstOrNull { it.key == selectedKey }
            ?: candidates.firstOrNull { it.extEnabled }
        val r = pick?.takeIf { it.candidate.kind == SourceKind.TORRENT && it.extEnabled } ?: return null
        val uri = r.candidate.torrentUri() ?: return null
        return Sendable(LinkSource.Torrent(uri, id.toString(), episode), r.key, "${r.candidate.extName} · ${r.candidate.label}", chosen = r.key == selectedKey)
    }

    // ---- parity wave A: details extras (15 §5 o.2) ----

    private fun loadDetails() {
        viewModelScope.launch {
            val result = runCatchingApp { metadata.details(ref) }
            _state.update {
                it.copy(details = result.fold(onSuccess = { d -> LoadState.Ok(d) }, onFailure = { e -> LoadState.Err(e.toAppError()) }))
            }
            updateAdultGate()
        }
    }

    /** Retry of the technical sheet alone (Info tab error). */
    fun retryDetails() {
        _state.update { it.copy(details = LoadState.Loading) }
        loadDetails()
    }

    private fun updateAdultGate() {
        val s = _state.value
        val media = (s.media as? LoadState.Ok)?.value ?: return
        val adult = s.detailsOrNull?.isAdult == true || media.looksAdult()
        val id = ref.anilistId
        val accepted = id != null && prefs.adultAccepted(id)
        val presentation = adultPresentation(s.matureContent, adult, accepted)
        _state.update {
            it.copy(
                adultGate = presentation == AdultPresentation.GATE,
                // Once revealed it stays revealed for this screen, even when the technical sheet re-runs the check.
                coverBlurred = presentation == AdultPresentation.BLUR && !revealedCover,
            )
        }
    }

    private var revealedCover = false

    /** BLUR mode: the first tap on the blurred banner / cover reveals them. */
    fun revealCover() {
        revealedCover = true
        _state.update { it.copy(coverBlurred = false) }
    }

    /** "Mostrar" of the 18+ gate: remembered for this title. */
    fun acceptAdult() {
        ref.anilistId?.let(prefs::acceptAdult)
        _state.update { it.copy(adultGate = false) }
    }

    fun setEpisodeQuery(query: String) = _state.update { it.copy(episodeQuery = query) }

    /** The page selector changes the view and remembers it (Diseño y estilo › Episodios, like the desktop). */
    fun setEpisodeView(view: EpisodeView) {
        prefs.setEpisodeView(view)
        _state.update { it.copy(episodeView = view) }
        settings?.let { repo -> viewModelScope.launch { runCatching { repo.update { it.copy(look = it.look.copy(episodeView = view)) } } } }
    }

    fun setRelationsView(view: RelationsView) {
        _state.update { it.copy(relationsView = view) }
        settings?.let { repo -> viewModelScope.launch { runCatching { repo.update { it.copy(look = it.look.copy(relationsView = view)) } } } }
    }

    /** Related tab: relations, seasons / watch order and recommendations, each once (again after an error). */
    fun loadRelated() {
        val s = _state.value
        if (s.relations == null || s.relations is LoadState.Err) {
            _state.update { it.copy(relations = LoadState.Loading) }
            viewModelScope.launch {
                val result = runCatchingApp { sortRelations(metadata.relations(ref)) }
                _state.update { it.copy(relations = result.toLoadState()) }
            }
        }
        if (s.seasons == null || s.seasons is LoadState.Err) {
            _state.update { it.copy(seasons = LoadState.Loading) }
            viewModelScope.launch {
                val result = runCatchingApp { metadata.seasons(ref) }
                _state.update { it.copy(seasons = result.toLoadState()) }
            }
        }
        if (s.recommendations == null || s.recommendations is LoadState.Err) {
            _state.update { it.copy(recommendations = LoadState.Loading) }
            viewModelScope.launch {
                val result = runCatchingApp { metadata.recommendations(ref).filter { r -> r.media.ref != ref } }
                _state.update { it.copy(recommendations = result.toLoadState()) }
            }
        }
    }

    /** Characters tab: first page + staff on first open, then the next page (infinite scroll / retry row). */
    fun loadCharacters() {
        val current = _state.value.characters ?: CharactersState()
        if (current.loading || !current.hasNext) return
        val page = current.page + 1
        _state.update { it.copy(characters = current.copy(loading = true, error = null)) }
        viewModelScope.launch {
            val result = runCatchingApp { metadata.characters(ref, page) }
            _state.update { s ->
                val c = s.characters ?: CharactersState()
                s.copy(
                    characters = result.fold(
                        onSuccess = { p ->
                            c.copy(
                                items = (c.items + p.items).distinctBy { e -> e.character.id },
                                page = page,
                                hasNext = p.hasNext,
                                loading = false,
                            )
                        },
                        onFailure = { e -> c.copy(loading = false, error = e.toAppError()) },
                    ),
                )
            }
        }
        val staff = _state.value.staff
        if (staff == null || staff is LoadState.Err) {
            _state.update { it.copy(staff = LoadState.Loading) }
            viewModelScope.launch {
                val result = runCatchingApp { metadata.staff(ref) }
                _state.update { it.copy(staff = result.toLoadState()) }
            }
        }
    }

    private fun <T> Result<T>.toLoadState(): LoadState<T> =
        fold(onSuccess = { LoadState.Ok(it) }, onFailure = { e -> LoadState.Err(e.toAppError()) })

    /**
     * [preferLocal]: a completed download of [episode] plays offline (parity wave B, r: downloads.local-library.play-offline) unless
     * the user picked a source in the sheet or plays on the PC.
     */
    private fun emitPlayer(
        episode: Int,
        sourceKey: String?,
        remoteSessionId: String? = null,
        preferLocal: Boolean = true,
        resume: Boolean = true,
        sourceConfirmed: Boolean = false,
    ) {
        val s = _state.value
        val title = (s.media as? LoadState.Ok)?.value?.title ?: return
        if (remoteSessionId == null) chosenInPanel = null
        val saved = s.progress[episode]
        val startMs = if (resume && saved != null && !saved.completed) saved.progressSec * 1000L else 0L
        val repo = downloads
        fun send(localUri: String?) {
            _events.trySend(
                DetailsEvent.NavigateToPlayer(
                    PlayerArgs(
                        media = ref,
                        title = title,
                        episode = episode,
                        startMs = startMs,
                        remoteSessionId = remoteSessionId,
                        sourceKey = sourceKey,
                        sourceConfirmed = sourceConfirmed && sourceKey != null,
                        localUri = localUri,
                    ),
                ),
            )
        }
        if (repo == null || !preferLocal || remoteSessionId != null || episode !in s.downloaded) {
            send(null)
            return
        }
        viewModelScope.launch { send(runCatching { repo.localUriFor(ref, episode) }.getOrNull()) }
    }
}
