// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.link.RemotePlaybackAccess
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.Container
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SkipMarksRepository
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.torrent.TorrentPlayback
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrentUri
import app.tatakai.next.player.ExternalLaunch
import app.tatakai.next.player.ExternalPlayer
import app.tatakai.next.player.FailKind
import app.tatakai.next.player.FailedSource
import app.tatakai.next.player.FlowPhase
import app.tatakai.next.player.HistoryFlush
import app.tatakai.next.player.Markers
import app.tatakai.next.player.NEXT_COUNTDOWN_SECS
import app.tatakai.next.player.PlayableSource
import app.tatakai.next.player.PlaybackControllerFactory
import app.tatakai.next.player.PlaybackListener
import app.tatakai.next.player.PlaybackState
import app.tatakai.next.player.PlayerSession
import app.tatakai.next.player.PlayerSessionFactory
import app.tatakai.next.player.ProgressTracker
import app.tatakai.next.player.RemotePlayback
import app.tatakai.next.player.SourceFlow
import app.tatakai.next.player.applyPick
import app.tatakai.next.player.magnetInfoHash
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import java.time.Instant

/** What the screen shows besides the engine's own [PlaybackState]. */
enum class PlayerPhase {
    /** Loading the media / the candidate list. */
    LOADING,

    /** Resolving a source (web stream or `TorrentRepository.play`). */
    RESOLVING,

    /** Engine loaded, no frame yet (or stalled). */
    BUFFERING,
    PLAYING,

    /** Manual mode: "Pick a source for this episode", nothing resolved yet. */
    MANUAL,

    /** Every source failed (or a direct source failed): ErrorState with `failed[]`. */
    ERROR,
    NO_SOURCES,
    REMOTE_UNAVAILABLE,

    /** The media / candidates could not be loaded (retryable). */
    FAILED,

    /** EXTERNAL engine: the informative screen. */
    EXTERNAL,
}

/** Torrent branch facts for the overlay, the seek bar and the "Open in the client" chip. */
data class TorrentUi(
    val torrentId: String?,
    val fileIndex: Int? = null,
    val fileName: String? = null,
    val batch: Boolean = false,
    val waitingMetadata: Boolean = true,
    val downBps: Long = 0,
    val peers: Int = 0,
    /** 0..1 of the selected files (`TorrentStats.progress`). */
    val progress: Double = 0.0,
    /** Downloaded byte ranges of the playing file (`seekMarks`). */
    val marks: List<ClosedFloatingPointRange<Double>> = emptyList(),
)

/** Metered network confirmation (11 §4.1 step 2): X = the source's size when known. */
data class MeteredPrompt(val sizeBytes: Long?, val hasWeb: Boolean)

data class NextEpisodeUi(val episode: Int, val secondsLeft: Int, val sameTorrent: Boolean)

data class ExternalUi(val launch: ExternalLaunch, val launched: Boolean = false, val failed: Boolean = false)

/** One-shot messages flashed over the video. */
sealed interface PlayerNotice {
    data object TryingNext : PlayerNotice
    data class NoFileNext(val episode: Int) : PlayerNotice
    data object IntroSkipped : PlayerNotice
    data object AssSimplified : PlayerNotice
}

data class PlayerUiState(
    val phase: PlayerPhase = PlayerPhase.LOADING,
    val engine: PlayerEngine = PlayerEngine.MEDIA3,
    val session: PlayerSession? = null,
    val poster: String? = null,
    val sources: List<RankedSource> = emptyList(),
    val candidatesLoading: Boolean = false,
    val candidatesError: AppError? = null,
    val mode: SourceMode = SourceMode.AUTO,
    val current: RankedSource? = null,
    /** `(n, total)` of "Source n of total". */
    val position: Pair<Int, Int>? = null,
    val failed: List<FailedSource> = emptyList(),
    val lastFailedTorrent: Boolean = false,
    val hasWeb: Boolean = false,
    /** Message of a failure outside the source flow (direct torrent / local file / media error). */
    val error: AppError? = null,
    val torrent: TorrentUi? = null,
    /** A torrent without a file for this episode: offer "Choose the file" in its client. */
    val noFileTorrentId: String? = null,
    val metered: MeteredPrompt? = null,
    val markers: Markers = Markers(),
    val hasNextEpisode: Boolean = false,
    val next: NextEpisodeUi? = null,
    val external: ExternalUi? = null,
    val settings: AppSettings = AppSettings(),
    /** The user can pick another source (a candidate list exists). */
    val canPickSource: Boolean = false,
    /** Not a Link (remote) session: episode navigation is available. */
    val remoteless: Boolean = true,
)

/**
 * Resolves what to play and drives one engine session (docs/redesign/12 §4 d):
 * - `remoteSessionId`: the Link HLS through the mTLS client (Media3 only);
 * - `localUri`: a completed download, offline, no source resolution;
 * - `torrentId` (+ `fileIndex`): a file of a torrent already in the session (batch next episode, client "Play this file");
 * - otherwise the ranked candidates of `ExtensionRepository.resolveCandidates` through [SourceFlow]: Auto = ranking with failover
 *   (torrents included) + `reportSourceResult`; Manual = nothing is resolved until the user picks; `PlayerArgs.sourceKey` first.
 * Progress goes to [HistoryRepository] (E9 keys). Survives rotation.
 */
@UnstableApi
class PlayerViewModel(
    private val args: PlayerArgs,
    private val metadata: MetadataRepository,
    private val extensions: ExtensionRepository,
    private val history: HistoryRepository,
    private val link: LinkRepository,
    private val remote: RemotePlaybackAccess,
    private val media3: PlaybackControllerFactory,
    private val flush: HistoryFlush?,
    private val settingsRepository: SettingsRepository? = null,
    private val torrents: TorrentRepository? = null,
    private val stats: SourceStatsStore? = null,
    private val skipMarks: SkipMarksRepository? = null,
    private val mpvFactory: PlayerSessionFactory? = null,
) : ViewModel() {
    private val settings: AppSettings = settingsRepository?.settings?.value ?: AppSettings()
    private val engine: PlayerEngine = when {
        args.remoteSessionId != null -> PlayerEngine.MEDIA3 // the PC's HLS needs the mTLS OkHttp client
        settings.playerEngine == PlayerEngine.MPV && mpvFactory == null -> PlayerEngine.MEDIA3
        else -> settings.playerEngine
    }

    private val _state = MutableStateFlow(
        PlayerUiState(engine = engine, settings = settings, mode = settings.sourcePrefs.mode, remoteless = args.remoteSessionId == null),
    )
    val state: StateFlow<PlayerUiState> = _state.asStateFlow()

    private val _notices = Channel<PlayerNotice>(Channel.BUFFERED)
    val notices: Flow<PlayerNotice> = _notices.receiveAsFlow()

    /** Another player entry to open in place of this one (next episode). */
    private val _replace = Channel<PlayerArgs>(Channel.CONFLATED)
    val replace: Flow<PlayerArgs> = _replace.receiveAsFlow()

    /** Survives `viewModelScope` cancellation: last progress row, `playbackEnded` and the remote `DELETE` must still go out on exit. */
    private val detached = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private var media: Media? = null
    private var session: PlayerSession? = null
    private var sessionJob: Job? = null
    private var flow: SourceFlow? = null
    private var resolveJob: Job? = null
    private var torrentJobs: List<Job> = emptyList()
    private var nextJob: Job? = null
    private var playback: TorrentPlayback? = null
    private var meteredAccepted = false
    private var loadsIssued = 0
    private var handledErrorLoad = 0
    private var handledStartLoad = 0
    private var autoSkippedIntro = false
    private var nextDismissed = false
    private var assNoticeShown = false
    private var lastPositionMs = 0L
    private var lastDurationMs = 0L
    private val mediaId: String? = HistoryEntry.animeIdOf(args.media.anilistId, args.media.malId)

    init {
        start()
    }

    // ---- start ------------------------------------------------------------------------------------------------------------------

    private fun start() {
        viewModelScope.launch {
            val remoteId = args.remoteSessionId
            if (remoteId != null) {
                val src = RemotePlayback.source(remote, remoteId, args.title)
                if (src == null) {
                    _state.update { it.copy(phase = PlayerPhase.REMOTE_UNAVAILABLE) }
                } else {
                    directLoad(src)
                }
                return@launch
            }
            val m = try {
                metadata.media(MediaKind.ANIME, args.media)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                null
            }
            media = m
            _state.update {
                it.copy(poster = m?.cover, hasNextEpisode = m?.episodes?.let { total -> args.episode < total } ?: true)
            }
            loadSkipMarks()
            val local = args.localUri
            val torrentId = args.torrentId
            when {
                local != null -> directLoad(
                    PlayableSource(url = local, headers = emptyMap(), container = Container.PROGRESSIVE, label = args.title),
                )
                m == null -> _state.update { it.copy(phase = PlayerPhase.FAILED, error = AppError.Unknown("media")) }
                torrentId != null -> playTorrentDirect(m, torrentId, args.fileIndex)
                else -> loadCandidates(m)
            }
        }
    }

    private fun loadSkipMarks() {
        val repo = skipMarks ?: return
        viewModelScope.launch {
            val marks = runCatching { repo.skipMarks(args.media, args.episode) }.getOrDefault(emptyList())
            _state.update { it.copy(markers = Markers.from(marks)) }
        }
    }

    private suspend fun loadCandidates(m: Media) {
        _state.update { it.copy(phase = PlayerPhase.LOADING, candidatesLoading = true, candidatesError = null) }
        val ranked = try {
            extensions.resolveCandidates(m, args.episode)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _state.update { it.copy(phase = PlayerPhase.FAILED, candidatesLoading = false, candidatesError = e.asAppError(), error = e.asAppError()) }
            return
        }
        val stored = mediaId?.let { id -> runCatching { stats?.pickedSource(id, args.episode) }.getOrNull() }
        val sources = applyPick(ranked, args.sourceKey, stored)
        _state.update { it.copy(candidatesLoading = false, sources = sources, canPickSource = sources.isNotEmpty()) }
        if (sources.isEmpty()) {
            _state.update { it.copy(phase = PlayerPhase.NO_SOURCES) }
            return
        }
        flow = SourceFlow(sources, settings.sourcePrefs.mode)
        advance()
    }

    // ---- source flow ------------------------------------------------------------------------------------------------------------

    /** Publishes the flow and resolves its current source when it asks for one. */
    private fun advance() {
        val f = flow ?: return
        publishFlow()
        val src = f.toResolve() ?: return
        val attempt = f.attempt
        resolveJob?.cancel()
        resolveJob = viewModelScope.launch { resolve(src, attempt) }
    }

    private fun publishFlow() {
        val f = flow ?: return
        _state.update {
            it.copy(
                phase = when (f.phase) {
                    FlowPhase.RESOLVING -> PlayerPhase.RESOLVING
                    FlowPhase.BUFFERING -> if (engine == PlayerEngine.EXTERNAL) PlayerPhase.EXTERNAL else PlayerPhase.BUFFERING
                    FlowPhase.PLAYING -> if (engine == PlayerEngine.EXTERNAL) PlayerPhase.EXTERNAL else PlayerPhase.PLAYING
                    FlowPhase.ERROR -> PlayerPhase.ERROR
                    FlowPhase.MANUAL -> PlayerPhase.MANUAL
                },
                sources = f.sources,
                current = f.currentSource(),
                position = f.position(),
                failed = f.failed,
                lastFailedTorrent = f.lastFailedTorrent(),
                hasWeb = f.bestWeb() != null,
                mode = f.mode,
            )
        }
    }

    private suspend fun resolve(src: RankedSource, attempt: Long) {
        val c = src.candidate
        // Leaving a torrent for another source: the after-watch policy decides what happens to it.
        val prev = playback
        if (prev != null && (c.kind != SourceKind.TORRENT || magnetInfoHash(c.torrentUri().orEmpty()) != prev.torrentId)) {
            endTorrent(completed = false)
        }
        when (c.kind) {
            SourceKind.WEB -> {
                val stream = try {
                    c.toStream()
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Throwable) {
                    null
                }
                if (stream == null) {
                    fail(attempt, FailKind.RESOLVE, "no stream")
                    return
                }
                handOff(PlayableSource.from(stream), attempt)
            }
            SourceKind.TORRENT -> resolveTorrent(src, attempt)
        }
    }

    private suspend fun resolveTorrent(src: RankedSource, attempt: Long) {
        val repo = torrents
        val m = media
        val uri = src.candidate.torrentUri()
        if (repo == null || m == null || uri == null) {
            fail(attempt, FailKind.UNKNOWN, "torrent engine unavailable")
            return
        }
        if (!meteredAccepted && repo.status.value.metered && settings.torrent.wifiOnly) {
            _state.update { it.copy(metered = MeteredPrompt(src.candidate.torrent?.sizeBytes, flow?.bestWeb() != null)) }
            return // resumed by acceptMetered() / declineMetered()
        }
        val hash = src.candidate.torrent?.infoHash?.lowercase() ?: magnetInfoHash(uri)
        _state.update { it.copy(torrent = TorrentUi(torrentId = hash)) }
        hash?.let { watchTorrent(it, null) }
        val result = repo.play(uri, m, args.episode)
        if (attempt != flow?.attempt) return
        result.fold(
            onSuccess = { p -> startTorrent(p, attempt) },
            onFailure = { e ->
                val err = e.asAppError()
                val noFile = noFileTorrent(err)
                if (noFile != null) _state.update { it.copy(noFileTorrentId = noFile) }
                fail(
                    attempt,
                    when {
                        noFile != null -> FailKind.NO_FILE
                        err is AppError.Offline -> FailKind.NO_PEERS
                        else -> FailKind.RESOLVE
                    },
                    e.message ?: err.toString(),
                )
            },
        )
    }

    private fun startTorrent(p: TorrentPlayback, attempt: Long) {
        playback = p
        _state.update {
            it.copy(
                torrent = TorrentUi(torrentId = p.torrentId, fileIndex = p.fileIndex, fileName = p.fileName, batch = p.batch, waitingMetadata = false),
            )
        }
        watchTorrent(p.torrentId, p.fileIndex)
        val source = PlayableSource(
            url = p.stream.url,
            headers = p.stream.headers,
            container = Container.PROGRESSIVE,
            label = p.fileName,
            quality = p.stream.quality,
            subtitles = p.stream.subtitles,
            torrent = true,
        )
        if (attempt < 0) directLoad(source) else handOff(source, attempt)
    }

    /** Collects the 1 s stats (speed / peers / %) and polls the seek-bar ranges every 2.5 s while this torrent plays. */
    private fun watchTorrent(id: String, fileIndex: Int?) {
        val repo = torrents ?: return
        torrentJobs.forEach { it.cancel() }
        val statsJob = viewModelScope.launch {
            try {
                repo.stats(id).collect { s ->
                    _state.update { st ->
                        st.copy(torrent = (st.torrent ?: TorrentUi(id)).copy(downBps = s.downBps, peers = s.peers, progress = s.progress))
                    }
                }
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                // stats of a torrent that is still being added may fail; the overlay simply shows no numbers
            }
        }
        val marksJob = if (fileIndex == null) null else viewModelScope.launch {
            while (isActive) {
                val marks = runCatching { repo.seekMarks(id, fileIndex) }.getOrNull()
                if (marks != null) _state.update { st -> st.copy(torrent = st.torrent?.copy(marks = marks)) }
                delay(SEEK_MARKS_POLL_MS)
            }
        }
        torrentJobs = listOfNotNull(statsJob, marksJob)
    }

    private fun endTorrent(completed: Boolean) {
        val p = playback ?: return
        playback = null
        torrentJobs.forEach { it.cancel() }
        torrentJobs = emptyList()
        _state.update { it.copy(torrent = null) }
        val repo = torrents ?: return
        detached.launch { runCatching { repo.playbackEnded(p.torrentId, completed) } }
    }

    /** The stream of [attempt] is ready: load it into the engine (or hand it to the external app). */
    private fun handOff(source: PlayableSource, attempt: Long) {
        val f = flow ?: return
        if (attempt != f.attempt) return
        f.resolved(attempt)
        if (engine == PlayerEngine.EXTERNAL) {
            launchExternal(source)
        } else {
            loadIntoSession(source)
        }
        publishFlow()
    }

    private fun fail(attempt: Long, kind: FailKind, detail: String) {
        val f = flow ?: return
        val before = f.phase
        val key = f.fail(attempt, kind, detail) ?: return
        viewModelScope.launch { runCatching { extensions.reportSourceResult(key, false) } }
        if (f.phase == FlowPhase.RESOLVING && before != FlowPhase.ERROR) _notices.trySend(PlayerNotice.TryingNext)
        advance()
    }

    // ---- direct sources (remote, local file, torrent already in the session) -----------------------------------------------------

    private fun directLoad(source: PlayableSource) {
        if (engine == PlayerEngine.EXTERNAL) {
            launchExternal(source)
            _state.update { it.copy(phase = PlayerPhase.EXTERNAL) }
        } else {
            loadIntoSession(source)
            _state.update { it.copy(phase = PlayerPhase.BUFFERING) }
        }
    }

    private suspend fun playTorrentDirect(m: Media, torrentId: String, fileIndex: Int?) {
        val repo = torrents
        if (repo == null) {
            _state.update { it.copy(phase = PlayerPhase.ERROR, error = AppError.Unknown("torrent engine unavailable")) }
            return
        }
        _state.update { it.copy(phase = PlayerPhase.RESOLVING, torrent = TorrentUi(torrentId = torrentId)) }
        watchTorrent(torrentId, null)
        repo.play(torrentId, m, args.episode, fileIndex).fold(
            onSuccess = { p -> startTorrent(p, attempt = -1) },
            onFailure = { e ->
                val err = e.asAppError()
                torrentJobs.forEach { it.cancel() }
                if (noFileTorrent(err) != null && fileIndex == null) {
                    // Batch next episode past the end of the pack: the normal source flow takes over (11 §4.2).
                    _notices.trySend(PlayerNotice.NoFileNext(args.episode))
                    _state.update { it.copy(torrent = null) }
                    loadCandidates(m)
                } else {
                    _state.update { it.copy(phase = PlayerPhase.ERROR, error = err, noFileTorrentId = noFileTorrent(err) ?: torrentId) }
                }
            },
        )
    }

    // ---- engine session ---------------------------------------------------------------------------------------------------------

    private fun ensureSession(): PlayerSession {
        session?.let { return it }
        val factory: PlayerSessionFactory = if (engine == PlayerEngine.MPV) mpvFactory ?: media3 else media3
        val s = factory.create(viewModelScope, listener(args.remoteSessionId), args.remoteSessionId != null)
        s.setSubtitleStyle(settings.subtitleStyle)
        s.setPreferredLanguages(settings.sourcePrefs.subLang, settings.sourcePrefs.dubLang)
        session = s
        _state.update { it.copy(session = s) }
        sessionJob = viewModelScope.launch { s.state.collect { onSessionState(it) } }
        return s
    }

    private fun loadIntoSession(source: PlayableSource) {
        val s = ensureSession()
        val resume = s.currentPositionMs().takeIf { loadsIssued > 0 && it > 0 } ?: args.startMs
        loadsIssued++
        autoSkippedIntro = false
        s.load(source, resume)
    }

    private fun onSessionState(st: PlaybackState) {
        if (st.loadId != loadsIssued || loadsIssued == 0) return
        if (st.positionMs > 0) lastPositionMs = st.positionMs
        if (st.durationMs > 0) lastDurationMs = st.durationMs
        val f = flow
        val err = st.error
        if (err != null && handledErrorLoad != st.loadId) {
            handledErrorLoad = st.loadId
            if (f != null && playbackInFlow()) {
                if (playback != null) endTorrent(completed = false)
                fail(f.attempt, FailKind.PLAYBACK, "${err.name} (${err.code})")
            } else {
                _state.update { it.copy(phase = PlayerPhase.ERROR, error = AppError.Unknown(err.name)) }
            }
            return
        }
        if (st.started && handledStartLoad != st.loadId) {
            handledStartLoad = st.loadId
            val key = f?.started()
            if (key != null) viewModelScope.launch { runCatching { extensions.reportSourceResult(key, true) } }
        }
        if (f != null && playbackInFlow()) {
            f.stalled(st.started && st.isBuffering)
            publishFlow()
        } else if (st.started) {
            _state.update { it.copy(phase = if (st.isBuffering) PlayerPhase.BUFFERING else PlayerPhase.PLAYING) }
        }
        if (st.subtitleIsAss && !st.fullAss && !assNoticeShown) {
            assNoticeShown = true
            _notices.trySend(PlayerNotice.AssSimplified)
        }
        // Settings › Reproductor › Saltar intro: jump over the intro once per load.
        if (settings.skipIntro && st.started && !autoSkippedIntro) {
            val target = _state.value.markers.skipTarget(st.positionMs / 1000.0)
            if (target != null) {
                autoSkippedIntro = true
                session?.seekTo((target * 1000).toLong())
                _notices.trySend(PlayerNotice.IntroSkipped)
            }
        }
        if (st.ended && settings.autoNext && _state.value.hasNextEpisode && nextJob == null && !nextDismissed && args.remoteSessionId == null) {
            startNextCountdown()
        }
    }

    /** The current load belongs to the source flow (not a direct torrent / local / remote load). */
    private fun playbackInFlow(): Boolean = flow?.phase.let { it == FlowPhase.BUFFERING || it == FlowPhase.PLAYING || it == FlowPhase.RESOLVING }

    private fun listener(remoteId: String?) = object : PlaybackListener {
        override fun onProgress(positionMs: Long, durationMs: Long, force: Boolean) = recordProgress(positionMs, durationMs, force)

        override fun onHeartbeat(positionMs: Long, paused: Boolean) {
            val id = remoteId ?: return
            detached.launch { remote.heartbeat(id, positionMs / 1000.0, paused) }
        }
    }

    private fun recordProgress(positionMs: Long, durationMs: Long, force: Boolean) {
        if (positionMs <= 0) return
        lastPositionMs = positionMs
        if (durationMs > 0) lastDurationMs = durationMs
        val entry = ProgressTracker.entry(
            anilistId = args.media.anilistId,
            malId = args.media.malId,
            title = args.title,
            poster = _state.value.poster,
            episode = args.episode,
            positionMs = positionMs,
            durationMs = durationMs,
            now = Instant.now(),
        ) ?: return
        detached.launch {
            runCatching { history.record(entry) }
            if (force) flush?.requestImmediate()
        }
    }

    // ---- external engine --------------------------------------------------------------------------------------------------------

    private fun launchExternal(source: PlayableSource) {
        val launch = ExternalPlayer.launch(source, args.title, args.startMs, settings.externalPlayerPackage)
        _state.update { it.copy(external = ExternalUi(launch)) }
    }

    /** The screen started the external activity ([ok] = false when no app could take it). */
    fun onExternalLaunched(ok: Boolean) {
        _state.update { st -> st.copy(external = st.external?.copy(launched = true, failed = !ok)) }
    }

    /** The external app returned: record the position it reported (mpv-android / VLC result extras). */
    fun onExternalResult(positionMs: Long?, durationMs: Long?) {
        if (positionMs != null && positionMs > 0) recordProgress(positionMs, durationMs ?: 0, force = true)
    }

    // ---- user actions -----------------------------------------------------------------------------------------------------------

    /** Picker / "Use the best web source": play [key] now (Manual and Auto) and remember it for this episode. */
    fun pickSource(key: String) {
        val f = flow ?: return
        mediaId?.let { id -> detached.launch { runCatching { stats?.setPickedSource(id, args.episode, key) } } }
        meteredAccepted = false
        _state.update { it.copy(metered = null, noFileTorrentId = null, error = null) }
        if (f.pick(key)) advance()
    }

    fun useBestWeb() {
        flow?.bestWeb()?.let { pickSource(it.key) }
    }

    /** "Watch anyway" on mobile data. */
    fun acceptMetered() {
        meteredAccepted = true
        _state.update { it.copy(metered = null) }
        val f = flow ?: return
        val src = f.toResolve() ?: return
        val attempt = f.attempt
        resolveJob?.cancel()
        resolveJob = viewModelScope.launch { resolve(src, attempt) }
    }

    /** "Pick a web source" (or dismiss) on mobile data. */
    fun declineMetered() {
        _state.update { it.copy(metered = null) }
        val f = flow ?: return
        val web = f.bestWeb()
        if (web != null) pickSource(web.key) else fail(f.attempt, FailKind.METERED_DECLINED, "metered")
    }

    fun retry() {
        val f = flow
        _state.update { it.copy(error = null, noFileTorrentId = null) }
        if (f != null) {
            f.retry()
            advance()
            return
        }
        val m = media
        val torrentId = args.torrentId
        when {
            args.remoteSessionId != null || args.localUri != null || m == null -> start()
            torrentId != null -> viewModelScope.launch { playTorrentDirect(m, torrentId, args.fileIndex) }
            else -> viewModelScope.launch { loadCandidates(m) }
        }
    }

    /** Reload the candidate list (picker error / empty state). */
    fun reloadSources() {
        val m = media ?: return start()
        if (flow == null) viewModelScope.launch { loadCandidates(m) }
    }

    fun seekBySkip() {
        val s = session ?: return
        val target = _state.value.markers.skipAt(s.currentPositionMs() / 1000.0)?.second ?: return
        s.seekTo((target * 1000).toLong())
    }

    private fun startNextCountdown() {
        val sameTorrent = _state.value.torrent?.batch == true && playback != null
        nextJob = viewModelScope.launch {
            for (left in NEXT_COUNTDOWN_SECS downTo 1) {
                _state.update { it.copy(next = NextEpisodeUi(args.episode + 1, left, sameTorrent)) }
                delay(1_000)
            }
            playNext()
        }
    }

    fun cancelNext() {
        nextJob?.cancel()
        nextDismissed = true
        _state.update { it.copy(next = null) }
    }

    /** "Next episode" (button or countdown): reuses the torrent of a batch (`play(torrentId, media, ep + 1)`), else the normal flow. */
    fun playNext() {
        nextJob?.cancel()
        _state.update { it.copy(next = null) }
        val p = playback
        val nextArgs = args.copy(
            episode = args.episode + 1,
            startMs = 0,
            sourceKey = null,
            torrentId = if (p != null && p.batch) p.torrentId else null,
            fileIndex = null,
            localUri = null,
        )
        _replace.trySend(nextArgs)
    }

    /** Pause before leaving for the torrent client ("Open in the client"). */
    fun pauseForClient() {
        session?.pause()
    }

    override fun onCleared() {
        session?.release() // emits the final forced progress tick
        session = null
        val completed = lastDurationMs > 0 && lastPositionMs >= lastDurationMs * COMPLETED_FRACTION
        endTorrent(completed)
        args.remoteSessionId?.let { id -> detached.launch { link.stop(id) } }
    }

    private fun Throwable.asAppError(): AppError = (this as? AppException)?.error ?: AppError.Unknown(message ?: javaClass.simpleName)

    private fun noFileTorrent(e: AppError): String? =
        (e as? AppError.Unknown)?.msg?.takeIf { it.startsWith(NO_FILE_PREFIX) }?.removePrefix(NO_FILE_PREFIX)?.ifBlank { null }

    private companion object {
        const val SEEK_MARKS_POLL_MS = 2_500L
        const val COMPLETED_FRACTION = 0.9
        const val NO_FILE_PREFIX = "torrent:no_file:"
    }
}
