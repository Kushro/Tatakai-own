// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import android.net.Uri
import android.os.SystemClock
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.link.RemotePlaybackAccess
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.Container
import app.tatakai.next.model.EmbedSniffException
import app.tatakai.next.model.EmbedSniffer
import app.tatakai.next.model.Episode
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SecretKeys
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SkipMarksRepository
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.TranslationMode
import app.tatakai.next.model.TranslationProvider
import app.tatakai.next.model.torrent.DebridRepository
import app.tatakai.next.model.torrent.TorrentPlayback
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrentUri
import app.tatakai.next.player.ActivePlayback
import app.tatakai.next.player.ExternalLaunch
import app.tatakai.next.player.ExternalPlayer
import app.tatakai.next.player.FailKind
import app.tatakai.next.player.FailedSource
import app.tatakai.next.player.FlowPhase
import app.tatakai.next.player.HistoryFlush
import app.tatakai.next.player.LanguageCodes
import app.tatakai.next.player.Markers
import app.tatakai.next.player.PlayableSource
import app.tatakai.next.player.PlaybackMetadata
import app.tatakai.next.player.PlaybackControllerFactory
import app.tatakai.next.player.PlaybackListener
import app.tatakai.next.player.PlaybackState
import app.tatakai.next.player.PlayerSession
import app.tatakai.next.player.PickMismatch
import app.tatakai.next.player.PlayerPrefs
import app.tatakai.next.player.PlayerSessionFactory
import app.tatakai.next.player.RemotePlayback
import app.tatakai.next.player.SleepOption
import app.tatakai.next.player.SleepTimer
import app.tatakai.next.player.SourceFlow
import app.tatakai.next.player.SubtitleDelay
import app.tatakai.next.player.SubtitleTools
import app.tatakai.next.player.SubtitleTranslationSession
import app.tatakai.next.player.TimedCue
import app.tatakai.next.player.TranslationCache
import app.tatakai.next.player.TranslationException
import app.tatakai.next.player.TranslatorConfig
import app.tatakai.next.player.embedFailDetail
import app.tatakai.next.player.historyAnimeId
import app.tatakai.next.player.magnetInfoHash
import app.tatakai.next.player.providerHistoryEntry
import app.tatakai.next.player.providerSources
import app.tatakai.next.player.resolveWebStream
import app.tatakai.next.player.markDebrid
import app.tatakai.next.player.resolvePick
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

/**
 * The end-of-episode card. [secondsLeft] null = auto-next is off (checklist player.auto-next.toggle): the card only offers the button and
 * never moves on by itself. [totalSecs] = `AppSettings.autoNextCountdownSecs` (3..30, player.auto-next.countdown).
 */
data class NextEpisodeUi(val episode: Int, val secondsLeft: Int?, val sameTorrent: Boolean, val totalSecs: Int = DEFAULT_COUNTDOWN_SECS)

internal const val DEFAULT_COUNTDOWN_SECS = 10

/** Sleep timer of the "More" sheet: [option] armed, [remainingMs] for the wall-clock options. */
data class SleepUi(val option: SleepOption, val remainingMs: Long?)

/** Episode list over the player (player.jump-to-episode). */
data class EpisodesUi(val loading: Boolean = false, val items: List<Episode> = emptyList(), val error: AppError? = null, val loaded: Boolean = false)

/** Subtitle translation state (player.subs.translate.*): [enabled] = the in-player toggle; [progress] = pre-translation (done, total). */
data class TranslationUi(
    val available: Boolean = false,
    val enabled: Boolean = true,
    val progress: Pair<Int, Int>? = null,
    val error: String? = null,
)

/**
 * Subtitles the screen draws itself (Media3 never renders text, mpv only when translating): [primary] parsed cues of the selected external
 * track or of a loaded file (null = the engine's own cues), [secondary] the second track (Media3), [translated] original -> translated.
 */
data class ScreenSubtitles(
    val primary: List<TimedCue>? = null,
    val secondary: List<TimedCue>? = null,
    val secondaryLabel: String? = null,
    val translated: Map<String, String> = emptyMap(),
)

/** A subtitle file loaded with "Load file…" while the Media3 engine plays (the screen draws it). */
data class CustomSubtitleUi(val name: String, val active: Boolean)

data class ExternalUi(val launch: ExternalLaunch, val launched: Boolean = false, val failed: Boolean = false)

/** One-shot messages flashed over the video. */
sealed interface PlayerNotice {
    data object TryingNext : PlayerNotice
    data class NoFileNext(val episode: Int) : PlayerNotice
    data object IntroSkipped : PlayerNotice
    data object AssSimplified : PlayerNotice

    // --- parity wave A (worker p) ---
    data class ScreenshotSaved(val path: String) : PlayerNotice
    data object ScreenshotFailed : PlayerNotice
    data class SubtitleLoaded(val name: String) : PlayerNotice
    data object SubtitleFailed : PlayerNotice
    data object SleepFired : PlayerNotice

    /** Bug sources.remembered-vs-prefs: the remembered source no longer fits the preferences and was dropped. */
    data class StoredSourceIgnored(val reason: PickMismatch) : PlayerNotice
    data class TranslationFailed(val message: String) : PlayerNotice
    data class PretranslateDone(val failed: Int) : PlayerNotice
    data object PretranslateUnavailable : PlayerNotice
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
    // --- parity wave A (worker p) ---
    val hasPrevEpisode: Boolean = false,
    val sleep: SleepUi? = null,
    val loop: Boolean = false,
    val episodes: EpisodesUi = EpisodesUi(),
    val translation: TranslationUi = TranslationUi(),
    val customSubtitle: CustomSubtitleUi? = null,
    /** External tracks of the current stream (Media3 secondary subtitle choices). */
    val externalSubtitles: List<app.tatakai.next.model.SubtitleTrack> = emptyList(),
    /** Index into [externalSubtitles] of the Media3 secondary track; mpv uses `PlaybackState.secondarySubtitleId`. */
    val secondaryExternal: Int? = null,
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
    // --- parity wave A (worker p) ---
    private val prefs: PlayerPrefs? = null,
    private val subtitleTools: SubtitleTools? = null,
    private val secrets: SecretStore? = null,
    private val debrid: DebridRepository? = null,
    // --- parity wave B (worker t): embed pages resolved in a hidden WebView (sources.embed-sniffer) ---
    private val embedSniffer: EmbedSniffer? = null,
) : ViewModel() {
    private val initialSettings: AppSettings = settingsRepository?.settings?.value ?: AppSettings()
    private val engine: PlayerEngine = when {
        args.remoteSessionId != null -> PlayerEngine.MEDIA3 // the PC's HLS needs the mTLS OkHttp client
        initialSettings.playerEngine == PlayerEngine.MPV && mpvFactory == null -> PlayerEngine.MEDIA3
        else -> initialSettings.playerEngine
    }

    private val _state = MutableStateFlow(
        PlayerUiState(
            engine = engine,
            settings = initialSettings,
            mode = initialSettings.sourcePrefs.mode,
            remoteless = args.remoteSessionId == null,
            hasPrevEpisode = args.remoteSessionId == null && args.episode > 1,
            translation = TranslationUi(available = initialSettings.subtitleTranslation.provider != TranslationProvider.NONE),
        ),
    )
    val state: StateFlow<PlayerUiState> = _state.asStateFlow()

    /** Live settings (Settings › Reproductor changes apply while playing: subtitle style, translation, countdown...). */
    private val settings: AppSettings get() = _state.value.settings

    private val _screenSubs = MutableStateFlow(ScreenSubtitles())
    val screenSubs: StateFlow<ScreenSubtitles> = _screenSubs.asStateFlow()

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
    // parity wave B (t, ext.custom-providers.continue): provider titles without AniList / MAL id use `ext:<ext>:<source>:<item>`.
    private val mediaId: String? = historyAnimeId(args.media.anilistId, args.media.malId, args.provider)

    // --- parity wave A (worker p) ---
    private var currentSource: PlayableSource? = null
    private val sleepTimer = SleepTimer()
    private var sleepJob: Job? = null
    private var customCues: List<TimedCue>? = null
    private var fetchedPrimaryFor: String? = null
    private var secondaryDecidedLoad = -1
    private var translationSession: SubtitleTranslationSession? = null
    private var translationFor: SubtitleTranslationConfigKey? = null
    private var liveJob: Job? = null
    private var pendingLive: List<String>? = null
    private var pretranslatedFor: String? = null
    private var pretranslateJob: Job? = null
    private var episodesJob: Job? = null
    private var notificationText: Pair<String, String?>? = null

    init {
        start()
        settingsRepository?.let { repo ->
            viewModelScope.launch {
                repo.settings.collect { s ->
                    val prev = _state.value.settings
                    _state.update {
                        it.copy(settings = s, translation = it.translation.copy(available = s.subtitleTranslation.provider != TranslationProvider.NONE))
                    }
                    val sess = session ?: return@collect
                    if (s.subtitleStyle != prev.subtitleStyle) sess.setSubtitleStyle(s.subtitleStyle)
                    if (s.stableVolume != prev.stableVolume) sess.setStableVolume(s.stableVolume)
                    if (s.subtitleTranslation != prev.subtitleTranslation) resetTranslation()
                }
            }
        }
        prefs?.let { p ->
            viewModelScope.launch { p.subtitleExtras.collect { e -> session?.setSubtitleExtras(e) } }
        }
    }

    /** Episode of this entry (episode list, screenshot names). */
    val episodeNumber: Int get() = args.episode

    /** Title used in screenshot file names. */
    val titleForFiles: String get() = args.title

    /** Media notification / lock screen text (PlaybackService): the screen passes the localised "Episode N" line. */
    fun setNotificationText(title: String, subtitle: String?) {
        notificationText = title to subtitle
        ActivePlayback.setMetadata(PlaybackMetadata(title, subtitle, _state.value.poster ?: media?.cover))
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
            // parity wave B (t, ext.custom-providers.watch): a provider title with no AniList id has nothing to ask the metadata
            // providers; a minimal Media carries the title and the next-episode button stays off.
            val providerOnly = args.provider != null && args.media.anilistId == null
            val m = if (providerOnly) {
                Media(
                    ref = args.media, kind = MediaKind.ANIME, title = args.title, titles = emptyMap(), cover = null, banner = null,
                    color = null, synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null,
                    score = null, sourceProvider = "extension",
                )
            } else {
                try {
                    metadata.media(MediaKind.ANIME, args.media)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    null
                }
            }
            media = m
            _state.update {
                it.copy(
                    poster = m?.cover,
                    hasNextEpisode = !providerOnly && (m?.episodes?.let { total -> args.episode < total } ?: true),
                )
            }
            notificationText?.let { (t, sub) -> ActivePlayback.setMetadata(PlaybackMetadata(t, sub, m?.cover)) }
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
        val provider = args.provider
        val ranked = try {
            // parity wave B (t, ext.custom-providers.watch): a provider entry plays the extension's own sources.
            if (provider != null) providerSources(extensions, provider) else extensions.resolveCandidates(m, args.episode)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            _state.update { it.copy(phase = PlayerPhase.FAILED, candidatesLoading = false, candidatesError = e.asAppError(), error = e.asAppError()) }
            return
        }
        val stored = if (provider != null) null else mediaId?.let { id -> runCatching { stats?.pickedSource(id, args.episode) }.getOrNull() }
        // 15 §3.9: torrent rows as the linked debrid services see them (identity without an account). Provider sources are web only.
        val marked = if (provider != null) ranked else markDebrid(ranked, debrid)
        // Bug sources.remembered-vs-prefs: this session's pick always wins; the remembered one only while it fits the preferences.
        val pick = resolvePick(marked, args.sourceKey, stored, settings.sourcePrefs)
        val sources = pick.sources
        if (pick.storedDiscarded) {
            mediaId?.let { id -> detached.launch { runCatching { stats?.clearPickedSource(id, args.episode) } } }
            pick.mismatch?.let { _notices.trySend(PlayerNotice.StoredSourceIgnored(it)) }
        }
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
                    // parity wave B (t, sources.embed-sniffer): embed pages go through the hidden WebView sniffer.
                    resolveWebStream(c, embedSniffer, settings.sourcePrefs)
                } catch (e: CancellationException) {
                    throw e
                } catch (e: EmbedSniffException) {
                    fail(attempt, FailKind.RESOLVE, embedFailDetail(e))
                    return
                } catch (e: Throwable) {
                    null
                }
                if (stream == null) {
                    fail(attempt, FailKind.RESOLVE, "no stream")
                    return
                }
                handOff(PlayableSource.from(stream), attempt)
            }
            SourceKind.TORRENT -> if (!resolveDebrid(src, attempt)) resolveTorrent(src, attempt)
        }
    }

    /**
     * 15 §3.9: a torrent cached by a linked debrid service plays as its direct HTTPS stream (no libtorrent). Returns false when the
     * normal torrent path must run (no service, not cached, or the service refused it).
     */
    private suspend fun resolveDebrid(src: RankedSource, attempt: Long): Boolean {
        val repo = debrid ?: return false
        val status = src.candidate.debrid?.takeIf { it.cached } ?: return false
        val hash = src.candidate.torrent?.infoHash ?: src.candidate.torrentUri()?.let { magnetInfoHash(it) } ?: return false
        val stream = try {
            repo.resolve(status.service, hash, src.candidate.torrentUri(), null, args.episode).getOrNull()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        } ?: return false
        if (attempt != flow?.attempt) return true
        handOff(PlayableSource.from(stream), attempt)
        return true
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
        prefs?.subtitleExtras?.value?.let { s.setSubtitleExtras(it) }
        s.setPreferredLanguages(settings.sourcePrefs.subLang, settings.sourcePrefs.dubLang)
        if (settings.stableVolume) s.setStableVolume(true)
        if (_state.value.loop) s.setLoop(true)
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
        currentSource = source
        customCues = null
        fetchedPrimaryFor = null
        pretranslatedFor = null
        _screenSubs.value = ScreenSubtitles()
        _state.update { it.copy(customSubtitle = null, externalSubtitles = source.subtitles, secondaryExternal = null) }
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
        onSubtitleTracks(st)
        if (st.ended && !st.loop && sleepTimer.onEpisodeEnded()) {
            onSleepFired()
            return
        }
        if (st.ended && !st.loop && !sleepTimer.fired && _state.value.hasNextEpisode && nextJob == null && _state.value.next == null &&
            !nextDismissed && args.remoteSessionId == null
        ) {
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
        val entry = providerHistoryEntry(
            anilistId = args.media.anilistId,
            malId = args.media.malId,
            provider = args.provider,
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

    /** Auto-next on: count `autoNextCountdownSecs` (3..30) and move on; off: the card only offers the button (player.auto-next.*). */
    private fun startNextCountdown() {
        val sameTorrent = _state.value.torrent?.batch == true && playback != null
        val total = settings.autoNextCountdownSecs.coerceIn(MIN_COUNTDOWN_SECS, MAX_COUNTDOWN_SECS)
        if (!settings.autoNext) {
            _state.update { it.copy(next = NextEpisodeUi(args.episode + 1, null, sameTorrent, total)) }
            return
        }
        nextJob = viewModelScope.launch {
            for (left in total downTo 1) {
                _state.update { it.copy(next = NextEpisodeUi(args.episode + 1, left, sameTorrent, total)) }
                delay(1_000)
            }
            playNext()
        }
    }

    fun cancelNext() {
        nextJob?.cancel()
        nextJob = null
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

    // ---- parity wave A (worker p): episodes, sleep timer, loop, stable volume ----------------------------------------------------

    /** "Previous episode" (button / J): N-1 through the normal source flow (its best source). */
    fun playPrevious() {
        if (!_state.value.hasPrevEpisode) return
        playEpisode(args.episode - 1)
    }

    /** Jump to [episode] from the episode list over the player, without going back to Details. */
    fun playEpisode(episode: Int) {
        if (episode < 1 || episode == args.episode || args.remoteSessionId != null) return
        nextJob?.cancel()
        _state.update { it.copy(next = null) }
        _replace.trySend(args.copy(episode = episode, startMs = 0, sourceKey = null, torrentId = null, fileIndex = null, localUri = null))
    }

    /** Loads the episode list once (Details' source: `MetadataRepository.episodes`, else 1..N from the media). */
    fun loadEpisodes(force: Boolean = false) {
        val cur = _state.value.episodes
        if (!force && (cur.loading || cur.loaded)) return
        episodesJob?.cancel()
        _state.update { it.copy(episodes = EpisodesUi(loading = true)) }
        episodesJob = viewModelScope.launch {
            val result = try {
                Result.success(metadata.episodes(args.media))
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                Result.failure(e)
            }
            val total = media?.episodes ?: 0
            val list = result.getOrNull().orEmpty().ifEmpty { (1..total).map { Episode(it, null, null, null) } }
            _state.update {
                it.copy(
                    episodes = if (list.isEmpty() && result.isFailure) {
                        EpisodesUi(error = result.exceptionOrNull()?.asAppError(), loaded = false)
                    } else {
                        EpisodesUi(items = list.sortedBy { e -> e.number }, loaded = true)
                    },
                )
            }
        }
    }

    fun setSleepTimer(option: SleepOption) {
        sleepTimer.set(option, SystemClock.elapsedRealtime())
        sleepJob?.cancel()
        sleepJob = null
        publishSleep()
        if (option.minutes != null) {
            sleepJob = viewModelScope.launch {
                while (isActive) {
                    delay(1_000)
                    if (sleepTimer.tick(SystemClock.elapsedRealtime())) {
                        onSleepFired()
                        break
                    }
                    publishSleep()
                }
            }
        }
    }

    private fun publishSleep() {
        val ui = if (sleepTimer.active) SleepUi(sleepTimer.option, sleepTimer.remainingMs(SystemClock.elapsedRealtime())) else null
        _state.update { it.copy(sleep = ui) }
    }

    /** The timer ran out: pause and do not move on to the next episode (player.sleep-timer). */
    private fun onSleepFired() {
        session?.pause()
        nextJob?.cancel()
        nextJob = null
        nextDismissed = true
        _state.update { it.copy(next = null, sleep = null) }
        _notices.trySend(PlayerNotice.SleepFired)
    }

    fun setLoop(loop: Boolean) {
        session?.setLoop(loop)
        _state.update { it.copy(loop = loop) }
        if (loop) {
            nextJob?.cancel()
            nextJob = null
            _state.update { it.copy(next = null) }
        }
    }

    fun setStableVolume(enabled: Boolean) {
        session?.setStableVolume(enabled)
        val repo = settingsRepository ?: return
        viewModelScope.launch { runCatching { repo.update { it.copy(stableVolume = enabled) } } }
    }

    // ---- parity wave A (worker p): subtitles ---------------------------------------------------------------------------------------

    /** Track choice from the subtitles sheet (null = off). A loaded file under Media3 is dropped when another track is chosen. */
    fun selectSubtitle(groupIndex: Int?) {
        if (_state.value.customSubtitle != null) {
            customCues = null
            _state.update { it.copy(customSubtitle = null) }
            _screenSubs.update { it.copy(primary = null) }
        }
        fetchedPrimaryFor = null
        session?.selectSubtitle(groupIndex)
    }

    /** ±0.1 s (player.subs.offset); [direction] 0 resets. */
    fun stepSubtitleDelay(direction: Int) {
        val s = session ?: return
        val cur = s.state.value.subtitleDelayMs
        s.setSubtitleDelay(if (direction == 0) 0 else SubtitleDelay.step(cur, direction))
    }

    /** "Load file…" (SAF): mpv adds and selects it (`sub-add`); Media3 draws its parsed cues itself. */
    fun loadSubtitleFile(uri: Uri) {
        val tools = subtitleTools ?: return
        viewModelScope.launch {
            val (file, cues) = try {
                tools.import(uri)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _notices.trySend(PlayerNotice.SubtitleFailed)
                return@launch
            }
            val s = session ?: return@launch
            if (!s.addSubtitleFile(file.absolutePath, file.name)) {
                s.selectSubtitle(null)
                _state.update { it.copy(customSubtitle = CustomSubtitleUi(file.name, active = true)) }
                _screenSubs.update { it.copy(primary = cues) }
            }
            customCues = cues
            pretranslatedFor = null
            _notices.trySend(PlayerNotice.SubtitleLoaded(file.name))
            maybePretranslate()
        }
    }

    /** Second subtitle (player.subs.secondary): mpv [groupIndex] (`secondary-sid`); Media3 [external] = index into the stream's tracks. */
    fun selectSecondary(groupIndex: Int?, external: Int?) {
        val s = session ?: return
        if (s.supportsSecondaryTracks) {
            s.selectSecondarySubtitle(groupIndex)
            return
        }
        _state.update { it.copy(secondaryExternal = external) }
        if (external == null) {
            _screenSubs.update { it.copy(secondary = null, secondaryLabel = null) }
            return
        }
        val track = currentSource?.subtitles?.getOrNull(external) ?: return
        val tools = subtitleTools ?: return
        viewModelScope.launch {
            val cues = runCatching { tools.fetch(track.url, currentSource?.headers.orEmpty(), track.format) }.getOrNull()
            if (cues == null) {
                _notices.trySend(PlayerNotice.SubtitleFailed)
                _state.update { it.copy(secondaryExternal = null) }
                return@launch
            }
            _screenSubs.update { it.copy(secondary = cues, secondaryLabel = track.label.ifBlank { track.lang }) }
        }
    }

    /**
     * Per engine update: (1) Media3 draws a selected external track from its parsed cues (exact ± delay, pre-translation); (2) the
     * preferred secondary language (`AppSettings.secondarySubLang`) is selected once per load.
     */
    private fun onSubtitleTracks(st: PlaybackState) {
        val s = session ?: return
        val src = currentSource ?: return
        if (!s.supportsSecondaryTracks && _state.value.customSubtitle == null) {
            val selected = st.subtitles.firstOrNull { it.selected }
            val ext = selected?.external?.let { src.subtitles.getOrNull(it) }
            val key = ext?.url
            if (key != fetchedPrimaryFor) {
                fetchedPrimaryFor = key
                if (ext == null) {
                    _screenSubs.update { it.copy(primary = null) }
                } else {
                    val tools = subtitleTools
                    if (tools != null) {
                        viewModelScope.launch {
                            val cues = runCatching { tools.fetch(ext.url, src.headers, ext.format) }.getOrNull()
                            if (fetchedPrimaryFor == key) {
                                _screenSubs.update { it.copy(primary = cues) }
                                if (cues != null) maybePretranslate()
                            }
                        }
                    }
                }
            }
        }
        val want = settings.secondarySubLang
        if (want != null && st.subtitles.isNotEmpty() && secondaryDecidedLoad != st.loadId) {
            secondaryDecidedLoad = st.loadId
            if (s.supportsSecondaryTracks) {
                st.subtitles.firstOrNull { !it.selected && LanguageCodes.matches(it.language, it.label, want) }
                    ?.let { s.selectSecondarySubtitle(it.groupIndex) }
            } else {
                val i = src.subtitles.indexOfFirst { LanguageCodes.matches(it.lang, it.label, want) }
                if (i >= 0) selectSecondary(null, i)
            }
        }
    }

    // ---- parity wave A (worker p): translation -------------------------------------------------------------------------------------

    fun setTranslationEnabled(enabled: Boolean) {
        _state.update { it.copy(translation = it.translation.copy(enabled = enabled, error = null)) }
        session?.setSubtitlesVisible(true)
    }

    /** Translation is configured and switched on in the player. */
    fun translationActive(): Boolean = _state.value.translation.let { it.available && it.enabled }

    private fun resetTranslation() {
        translationSession = null
        translationFor = null
        pretranslatedFor = null
        _screenSubs.update { it.copy(translated = emptyMap()) }
    }

    /** One translation session per (provider settings, key) and episode; the key comes from SecretStore, never from settings. */
    private suspend fun translation(): SubtitleTranslationSession? {
        val tools = subtitleTools ?: return null
        val cfg = settings.subtitleTranslation
        if (cfg.provider == TranslationProvider.NONE) return null
        val key = runCatching { secrets?.get(SecretKeys.translationApiKey(cfg.provider.id)) }.getOrNull()
        val id = SubtitleTranslationConfigKey(cfg, key?.hashCode())
        translationSession?.takeIf { translationFor == id }?.let { return it }
        val config = TranslatorConfig.from(cfg, key)
        val translator = try {
            tools.translator(config)
        } catch (e: TranslationException) {
            _state.update { it.copy(translation = it.translation.copy(error = e.message)) }
            _notices.trySend(PlayerNotice.TranslationFailed(e.message.orEmpty()))
            return null
        }
        val episodeKey = "${mediaId ?: args.title}:${args.episode}"
        return SubtitleTranslationSession(translator, tools.translationCache, TranslationCache.scope(episodeKey, config)).also {
            translationSession = it
            translationFor = id
        }
    }

    /**
     * Live translation (player.subs.translate.live): the screen hands the lines it is about to draw; cached ones come back at once, the
     * rest are translated (one request in flight, the newest lines win). Until then the original is shown.
     */
    fun translateLines(lines: List<String>) {
        if (!translationActive() || lines.isEmpty()) return
        val known = _screenSubs.value.translated
        if (lines.all { it in known }) return
        pendingLive = lines
        if (liveJob?.isActive == true) return
        liveJob = viewModelScope.launch {
            while (true) {
                val batch = pendingLive ?: break
                pendingLive = null
                val tr = translation() ?: break
                val missing = batch.filter { it !in _screenSubs.value.translated }
                if (missing.isEmpty()) continue
                try {
                    val out = tr.translateLive(missing)
                    _screenSubs.update { it.copy(translated = it.translated + missing.zip(out)) }
                } catch (e: CancellationException) {
                    throw e
                } catch (e: Exception) {
                    _state.update { it.copy(translation = it.translation.copy(error = e.message)) }
                    break
                }
            }
        }
    }

    /** mpv draws nothing while the screen shows its translated line (and everything again when translation is off). */
    fun onTranslationDrawing(drawing: Boolean) {
        session?.setSubtitlesVisible(!drawing)
    }

    /** Settings mode PRELOAD: translate the whole parsed track before playing (once per track). */
    private fun maybePretranslate() {
        if (settings.subtitleTranslation.mode == TranslationMode.PRELOAD && translationActive()) pretranslate()
    }

    /**
     * "Pre-translate" (player.subs.translate.pretranslate): pauses, translates every line of the current track in batches with progress,
     * then plays on. Needs the full text: a loaded file or an external track (embedded tracks only translate live).
     */
    fun pretranslate() {
        if (pretranslateJob?.isActive == true) return
        pretranslateJob = viewModelScope.launch {
            val cues = cuesForPretranslation()
            if (cues == null) {
                _notices.trySend(PlayerNotice.PretranslateUnavailable)
                return@launch
            }
            val id = cues.size.toString() + ":" + cues.firstOrNull()?.text.orEmpty()
            if (pretranslatedFor == id) return@launch
            val tr = translation() ?: return@launch
            val s = session
            val wasPlaying = s?.state?.value?.isPlaying == true
            s?.pause()
            val texts = cues.map { it.text }
            val failed = try {
                tr.pretranslate(texts) { done, total -> _state.update { it.copy(translation = it.translation.copy(progress = done to total)) } }
            } catch (e: CancellationException) {
                _state.update { it.copy(translation = it.translation.copy(progress = null)) }
                throw e
            }
            val all = texts.distinct()
            val cached = tr.cachedOnly(all)
            _screenSubs.update { sub -> sub.copy(translated = sub.translated + all.zip(cached).mapNotNull { (o, t) -> t?.let { o to it } }) }
            pretranslatedFor = id
            _state.update { it.copy(translation = it.translation.copy(progress = null)) }
            _notices.trySend(PlayerNotice.PretranslateDone(failed))
            if (wasPlaying) s?.play()
        }
    }

    /** "Skip" on the pre-translation overlay: play now, the rest is translated live. */
    fun cancelPretranslate() {
        pretranslateJob?.cancel()
        _state.update { it.copy(translation = it.translation.copy(progress = null)) }
        session?.play()
    }

    private suspend fun cuesForPretranslation(): List<TimedCue>? {
        customCues?.let { return it }
        _screenSubs.value.primary?.let { return it }
        val src = currentSource ?: return null
        val tools = subtitleTools ?: return null
        val selected = session?.state?.value?.subtitles?.firstOrNull { it.selected } ?: return null
        val track = selected.external?.let { src.subtitles.getOrNull(it) }
            ?: src.subtitles.firstOrNull { LanguageCodes.matches(it.lang, it.label, selected.language ?: selected.label) }
            ?: return null
        return runCatching { tools.fetch(track.url, src.headers, track.format) }.getOrNull()
    }

    // ---- parity wave A (worker p): screenshots -------------------------------------------------------------------------------------

    fun onScreenshot(path: String?) {
        _notices.trySend(if (path != null) PlayerNotice.ScreenshotSaved(path) else PlayerNotice.ScreenshotFailed)
    }

    /** Pause before leaving for the torrent client ("Open in the client"). */
    fun pauseForClient() {
        session?.pause()
    }

    override fun onCleared() {
        sleepJob?.cancel()
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
        const val MIN_COUNTDOWN_SECS = 3
        const val MAX_COUNTDOWN_SECS = 30
        const val SEEK_MARKS_POLL_MS = 2_500L
        const val COMPLETED_FRACTION = 0.9
        const val NO_FILE_PREFIX = "torrent:no_file:"
    }
}

/** Identity of a translation setup (the key itself is never kept, only whether / which one is set). */
private data class SubtitleTranslationConfigKey(val settings: app.tatakai.next.model.SubtitleTranslation, val keyHash: Int?)
