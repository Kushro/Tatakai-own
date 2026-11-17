// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.feature.player.sources.rankForThisDevice
import android.net.Uri
import android.os.SystemClock
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.link.LinkApiError
import app.tatakai.next.link.LinkError
import app.tatakai.next.link.RemotePlaybackAccess
import app.tatakai.next.model.LinkProfile
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkSessionStatus
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LocalEnhancementGate
import app.tatakai.next.model.PhoneFrameGenGate
import app.tatakai.next.model.ProcessingLocation
import app.tatakai.next.model.RemoteSession
import app.tatakai.next.model.groupedLinkProfiles
import app.tatakai.next.model.linkProcessingInfo
import app.tatakai.next.model.mixedLinkProfile
import app.tatakai.next.model.playableLinkProfile
import app.tatakai.next.model.ProfileSupport
import app.tatakai.next.model.VideoDecodeLimits
import app.tatakai.next.model.linkProfilesMayDropFrames
import app.tatakai.next.model.unsupportedLinkProfiles
import app.tatakai.next.link.linkErrorOrNull
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
import app.tatakai.next.player.ShaderFallback
import app.tatakai.next.player.PlaybackError
import app.tatakai.next.player.decodesInHardware
import app.tatakai.next.player.decodeRecovery
import app.tatakai.next.player.switchToMedia3OnStall
import app.tatakai.next.player.DeviceCodecs
import app.tatakai.next.player.DecodeRecovery
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
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

    /** BUG-020: Media3 could not decode this video; it plays with libmpv (software decoding, may be slow). */
    data object SwitchedToMpv : PlayerNotice

    /** BUG-059: libmpv showed no frame of this stream in time; it plays with Media3 (no shader upscaling). */
    data object SwitchedToMedia3 : PlayerNotice

    /** BUG-043 / BUG-006: libmpv changed the shader preset by itself (too slow, or low battery). */
    data class ShaderChanged(val kind: ShaderFallback) : PlayerNotice

    /** BUG-038: the PC had closed the Link session; a new one continues at the same position. */
    data object LinkSessionRenewed : PlayerNotice

    /** BUG-022 / BUG-131: how a Link profile change ended (changed, same profile, busy, or why it failed). */
    data class LinkProfile(val result: LinkProfileResult) : PlayerNotice

    /** BUG-131: the renewed Link session did not start in time and was opened again once. */
    data object LinkProfileRetried : PlayerNotice

    /** BUG-132: this phone's decoder could not play the profile; the session went back to [to] (null = its name is unknown). */
    data class LinkPhoneCannotPlay(val to: app.tatakai.next.model.LinkProfile?) : PlayerNotice

    /** BUG-132: the saved preferred profile [from] is not playable on this phone; [to] is used (and saved) instead. */
    data class LinkPreferredReplaced(val from: app.tatakai.next.model.LinkProfile, val to: app.tatakai.next.model.LinkProfile) : PlayerNotice

    /**
     * 0.4.0 "Procesar en": a switch to [to] happened ([failure] and [block] null), failed ([failure]: the video goes on where it
     * was) or was refused ([block], [detail]). [onPhoneInstead]: the player was asked to start at [to] (Settings default, the
     * previous episode) and plays on the phone instead.
     */
    data class Processing(
        val to: ProcessingLocation,
        val failure: ProcessingFailure? = null,
        val block: ProcessingBlock? = null,
        val detail: String? = null,
        val onPhoneInstead: Boolean = false,
    ) : PlayerNotice
}

/**
 * BUG-038 / BUG-009 / BUG-058: whether recovering a Link session opens a new one. [alive] = whether the PC still has the session
 * (null: the PC could not be asked). A session the PC closed is always renewed; one it still has only on the user's Retry
 * (reloading a stuck session fails again), never on the automatic path. Needs a session the phone can renew ([canRenew]).
 */
internal fun shouldRenewRemote(alive: Boolean?, canRenew: Boolean, auto: Boolean): Boolean =
    canRenew && (alive == false || (alive == true && !auto))

/**
 * BUG-138: whether start() renews a Link session before loading it: only when the PC said it no longer has it (a hand-off
 * card tapped after the 120 s idle timeout). Alive or unknown (PC unreachable, slow answer) loads it as before, so a network
 * drop still goes through the retries and the "connection lost" screen.
 */
internal fun renewRemoteBeforeLoad(alive: Boolean?, canRenew: Boolean): Boolean = canRenew && alive == false

/** BUG-038 / BUG-026: why a Link (PC) playback stopped, for an error screen in words (no Media3 codes, no "Extensions"). */
enum class RemoteFailure {
    /** The PC did not answer (network cut, PC asleep): playback resumes by itself when the network returns. */
    CONNECTION_LOST,

    /** The PC closed the session and it cannot be recreated here (the PC sent it): send it again from the PC. */
    SESSION_ENDED,

    /** The PC answered with an error for a session it still has. */
    PC_ERROR,

    /** BUG-132: this phone's video decoder does not take the profile's stream (not the PC's fault); Retry uses another profile. */
    PHONE_DECODER,
}

data class PlayerUiState(
    val phase: PlayerPhase = PlayerPhase.LOADING,
    /** BUG-082 parity: with NO_SOURCES, whether some extension (or the PC's server) was asked: "no sources found" vs "install one". */
    val sourcesAsked: Boolean = false,
    val engine: PlayerEngine = PlayerEngine.MEDIA3,
    val session: PlayerSession? = null,
    val poster: String? = null,
    val sources: List<RankedSource> = emptyList(),
    val candidatesLoading: Boolean = false,
    val candidatesError: AppError? = null,
    val mode: SourceMode = SourceMode.AUTO,
    /** BUG-140: the remembered pick for this episode ([RankedSource.key]); the picker says when its list does not have it. */
    val rememberedPick: String? = null,
    val current: RankedSource? = null,
    /** `(n, total)` of "Source n of total". */
    val position: Pair<Int, Int>? = null,
    val failed: List<FailedSource> = emptyList(),
    val lastFailedTorrent: Boolean = false,
    val hasWeb: Boolean = false,
    /** BUG-020: sources still to try (enabled, not failed); the error screen only says "no other source" at 0. */
    val remaining: Int = 0,
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
    /** Not playing from the PC (no Link session now). 0.4.0: episode navigation is [canNavigateEpisodes]. */
    val remoteless: Boolean = true,
    /**
     * 0.4.0: next / previous / episode list are available: always on the phone, and from the PC too when the playback has a
     * title the phone can look up (a switch to PC from the player, a hand-off of an AniList title); not for a PC library file.
     */
    val canNavigateEpisodes: Boolean = true,
    /** BUG-038 / BUG-026: set with phase ERROR when a Link session failed. */
    val remoteFailure: RemoteFailure? = null,
    /** BUG-022: the PC's enhancement profiles (Link sessions only) and the one this session uses. */
    val linkProfiles: List<LinkProfile> = emptyList(),
    val linkProfile: String? = null,
    /** BUG-022: a profile change is restarting the session. */
    val linkProfileChanging: Boolean = false,
    /** BUG-132: the profiles of [linkProfiles] this phone cannot play (disabled in the sheet, with the reason). */
    val linkUnsupported: Map<String, ProfileSupport> = emptyMap(),
    /** BUG-135: the profiles of [linkProfiles] above the decoder's declared frame rate (selectable, with a note). */
    val linkMayDropFrames: Set<String> = emptySet(),
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
    /** BUG-132: what the phone's H.264 decoder plays (the Link profiles it cannot are disabled); null = unknown. */
    private val linkDecodeLimits: () -> VideoDecodeLimits? = { app.tatakai.next.link.AvcDecodeLimits.device() },
    // --- 0.4.0 (worker C): Phone | PC | Mixto. No-ops until the coordinator binds worker A's / B's implementations ---
    /** Worker A's quality engine: local shaders off while the PC processes. */
    private val enhancementGate: LocalEnhancementGate = LocalEnhancementGate.NONE,
    /** Worker B's phone frame generation: eligibility for "Mixto", on in Mixto, off in strict PC. */
    private val frameGenGate: PhoneFrameGenGate = PhoneFrameGenGate.NONE,
) : ViewModel() {
    private val initialSettings: AppSettings = settingsRepository?.settings?.value ?: AppSettings()

    /** 0.4.0: the engine of the phone's own playback (the setting; BUG-020 / BUG-059 may change it for this player). */
    private var localEngine: PlayerEngine = when {
        initialSettings.playerEngine == PlayerEngine.MPV && mpvFactory == null -> PlayerEngine.MEDIA3
        else -> initialSettings.playerEngine
    }

    /** Can change once, from Media3 to libmpv, when Media3 cannot decode the video (BUG-020, [switchToMpv]). */
    private var engine: PlayerEngine = when {
        args.remoteSessionId != null -> PlayerEngine.MEDIA3 // the PC's HLS needs the mTLS OkHttp client
        else -> localEngine
    }

    /** 0.4.0: where the video is processed now; a PC hand-off / "Enviar a dispositivo" starts on the PC. */
    private var location: ProcessingLocation = if (args.remoteSessionId != null) ProcessingLocation.PC else ProcessingLocation.PHONE

    /** 0.4.0: the phone can look up this title (episodes, and playing it on the phone from a PC session). */
    private val titleKnown: Boolean = args.media.anilistId != null || args.media.malId != null || args.provider != null

    private val _state = MutableStateFlow(
        PlayerUiState(
            engine = engine,
            settings = initialSettings,
            mode = initialSettings.sourcePrefs.mode,
            remoteless = args.remoteSessionId == null,
            canNavigateEpisodes = args.remoteSessionId == null || titleKnown,
            hasPrevEpisode = (args.remoteSessionId == null || titleKnown) && args.episode > 1,
            translation = TranslationUi(available = initialSettings.subtitleTranslation.provider != TranslationProvider.NONE),
        ),
    )
    val state: StateFlow<PlayerUiState> = _state.asStateFlow()

    private val _processing = MutableStateFlow(ProcessingUiState(location = location))

    /**
     * 0.4.0 "Procesar en: Teléfono | PC | Mixto" for worker D's UI: where the video is processed, the options with their block
     * reasons, a switch in progress, what the PC does and its profiles. Commands: [setProcessingLocation], [setLinkProfile].
     */
    val processing: StateFlow<ProcessingUiState> = _processing.asStateFlow()

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
    /** BUG-038: the Link session being played; a renewed session replaces [PlayerArgs.remoteSessionId]. */
    private var remoteId: String? = args.remoteSessionId
    private var remoteRecovery: Job? = null

    /** BUG-131: one profile change at a time, bounded, with a result for every tap ([LinkProfileChange]). */
    private val profileChange = LinkProfileChange(
        scope = viewModelScope,
        renew = { id, startSecs, profile -> remote.renew(id, startSecs, profile) },
        ready = { id -> remote.sessionReady(id) },
        log = { line -> runCatching { android.util.Log.i(LINK_LOG_TAG, line) } },
    )

    /** BUG-132: Link profiles this phone's decoder already failed with in this player (never asked again, Retry included). */
    private val rejectedProfiles = mutableSetOf<String>()

    /** BUG-132: the profile used before the last profile change (where a decoder rejection goes back to). */
    private var profileBeforeChange: String? = null
    private var linkDefaultProfile: String? = null
    private var linkLimits: VideoDecodeLimits? = null

    /** BUG-131: waits for a renewed Link session to show its first frame ([watchRenewedStart]). */
    private var renewWatch: Job? = null
    private var handledErrorLoad = 0
    private var handledStartLoad = 0

    /** BUG-020 / BUG-042: silent decoder retries spent on [decodeRetryKey] (the flow attempt, or the direct source's URL). */
    private var shaderNoticeSeq = 0
    private var decodeRetries = 0
    private var decodeRetryKey: String? = null

    /** BUG-059: URL of the source already moved from libmpv to Media3 (it is not moved twice). */
    private var media3TriedFor: String? = null
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

    // --- 0.4.0 (worker C): Phone | PC | Mixto ---
    /** The location this player was asked to start at when it is not the phone (Settings default, the previous episode). */
    private var pendingStart: ProcessingLocation? = null

    /** The phone source of the last local load (played again on "PC -> Phone"; its URL / torrent goes to the PC). */
    private var localSource: PlayableSource? = null

    /** A torrent sent to the PC before the phone played it ([pendingStart] with a torrent candidate). */
    private var pendingLinkSource: LinkSource? = null

    /** The position the next load starts at ("PC -> Phone" through the source flow). */
    private var pendingResumeMs: Long? = null

    /** The phone's audio language / label before going to the PC, selected again when the phone plays the source again. */
    private var pendingAudio: Pair<String?, String?>? = null
    private var audioRestoredLoad = -1

    /** The PC profile of strict PC (Mixto uses an upscale-only one; going back to PC brings this one back). */
    private var pcProfile: String? = null
    private var capabilities: LinkCapabilities? = null
    private var lastStatus: LinkSessionStatus? = null
    private var statusJob: Job? = null

    private val switcher = ProcessingSwitcher(
        scope = viewModelScope,
        port = object : ProcessingSwitcher.Port {
            override fun positionMs(): Long = positionNow()
            override fun audioTrackIndex(): Int? = session?.state?.value?.audioTracks?.indexOfFirst { it.selected }?.takeIf { it > 0 }
            override suspend fun currentLinkSource(): LinkSource? = currentLinkSourceOrNull()
            override suspend fun startAt(source: LinkSource, profileId: String?, startSecs: Double, audioTrack: Int?) =
                remote.startAt(source, profileId, startSecs, audioTrack)
            override suspend fun renew(sessionId: String, startSecs: Double, profileId: String?) = remote.renew(sessionId, startSecs, profileId)
            override suspend fun ready(sessionId: String): Boolean? = remote.sessionReady(sessionId)
            override fun playRemote(location: ProcessingLocation, session: RemoteSession, profileId: String?): Boolean =
                playOnPc(location, session.sessionId, profileId)
            override fun playLocal(positionMs: Long): Boolean = playOnPhone(positionMs)
            override fun stop(sessionId: String) {
                detached.launch { runCatching { link.stop(sessionId) } }
            }
        },
        log = { line -> runCatching { android.util.Log.i(LINK_LOG_TAG, line) } },
    )

    init {
        if (args.remoteSessionId == null) {
            val paired = link.state.value is LinkState.Paired
            val carried = ProcessingCarry.take(titleKey)
            pendingStart = startLocation(false, carried, link.linkedDevice.value?.defaultProcessing, paired).takeIf { it != ProcessingLocation.PHONE }
        }
        start()
        applyGates()
        viewModelScope.launch {
            link.state.collect { s ->
                if (s is LinkState.Paired && s.reachable && capabilities == null) loadCapabilities()
                refreshProcessing()
            }
        }
        viewModelScope.launch { frameGenGate.availability.collect { refreshProcessing() } }
        viewModelScope.launch { switcher.switching.collect { refreshProcessing() } }
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
            val remoteId = this@PlayerViewModel.remoteId
            if (remoteId != null) {
                loadLinkProfiles(remoteId)
                startStatusPolling() // 0.4.0: "En la PC" says what the PC does
                launch { loadRemotePoster() }
                // BUG-138: a hand-off card tapped after the PC closed the idle session (120 s): renew it now instead of
                // loading a session that answers 410 (Media3 retried it ~65 s before the recovery renewed it)
                val alive = withTimeoutOrNull(START_ALIVE_CHECK_TIMEOUT_MS) { remote.sessionAlive(remoteId) }
                if (renewRemoteBeforeLoad(alive, remote.canRenew(remoteId))) {
                    runCatching { android.util.Log.i(LINK_LOG_TAG, "session=$remoteId already closed by the PC: renewing before load") }
                    recoverRemote(auto = true)
                    return@launch
                }
                val src = remoteSourceFor(location, remoteId)
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

    /**
     * BUG-096: a Link session skips the source flow (and its metadata), so its history rows were saved without a poster and
     * "Continuar viendo" lost the cover. Only the poster (and the notification artwork) is taken: the session stays remote.
     */
    private suspend fun loadRemotePoster() {
        if (args.media.anilistId == null && args.media.malId == null) return
        val m = try {
            metadata.media(MediaKind.ANIME, args.media)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            null
        }
        // 0.4.0: the title is known, so "PC -> Phone" can play it here and the next episode button works from the PC too
        if (m != null && media == null) {
            media = m
            _state.update { it.copy(hasNextEpisode = m.episodes?.let { total -> args.episode < total } ?: true) }
            refreshProcessing()
        }
        val cover = m?.cover ?: return
        _state.update { it.copy(poster = cover) }
        notificationText?.let { (t, sub) -> ActivePlayback.setMetadata(PlaybackMetadata(t, sub, cover)) }
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
        if (provider != null) {
            // parity wave B (t, ext.custom-providers.watch): a provider entry plays the extension's own sources (web only).
            val ranked = try {
                providerSources(extensions, provider)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _state.update { it.copy(phase = PlayerPhase.FAILED, candidatesLoading = false, candidatesError = e.asAppError(), error = e.asAppError()) }
                return
            }
            startFlow(resolvePick(ranked, args.sourceKey, null, settings.sourcePrefs).sources)
            _state.update { it.copy(candidatesLoading = false) }
            return
        }
        val stored = mediaId?.let { id -> runCatching { stats?.pickedSource(id, args.episode) }.getOrNull() }
        _state.update { it.copy(rememberedPick = stored) }
        var noticeSent = false
        fun prepare(ranked: List<RankedSource>): List<RankedSource> {
            // Bug sources.remembered-vs-prefs: this session's pick always wins; the remembered one only while it fits the preferences.
            val pick = resolvePick(rankForThisDevice(ranked), args.sourceKey, stored, settings.sourcePrefs)
            if (pick.storedDiscarded && !noticeSent && ranked.any { it.key == stored }) {
                noticeSent = true
                mediaId?.let { id -> detached.launch { runCatching { stats?.clearPickedSource(id, args.episode) } } }
                _state.update { it.copy(rememberedPick = null) }
                pick.mismatch?.let { _notices.trySend(PlayerNotice.StoredSourceIgnored(it)) }
            }
            return pick.sources
        }
        // BUG-011 / BUG-019: sources arrive progressively. Manual shows the picker with the first answers and fills it in; Auto starts
        // when everything answered, or after AUTO_START_MS with what is there (later answers still join the list for the picker).
        var latest: List<RankedSource> = emptyList()
        val manual = settings.sourcePrefs.mode == SourceMode.MANUAL
        val failure = try {
            coroutineScope {
                val timer = if (manual) null else launch {
                    delay(AUTO_START_MS)
                    if (flow == null && latest.isNotEmpty()) startFlow(markDebrid(latest, debrid))
                }
                extensions.candidatesFlow(m, args.episode).collect { partial ->
                    latest = prepare(partial)
                    val f = flow
                    when {
                        f != null -> {
                            f.updateSources(latest)
                            publishFlow()
                        }
                        manual && latest.isNotEmpty() -> startFlow(latest)
                        else -> _state.update { it.copy(sources = latest, canPickSource = latest.isNotEmpty()) }
                    }
                }
                timer?.cancel()
            }
            null
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            e
        }
        // 15 §3.9: torrent rows as the linked debrid services see them (identity without an account).
        val final = markDebrid(latest, debrid)
        val f = flow
        if (f != null) {
            f.updateSources(final)
            _state.update { it.copy(candidatesLoading = false) }
            publishFlow()
            return
        }
        if (final.isEmpty() && failure != null) {
            _state.update { it.copy(phase = PlayerPhase.FAILED, candidatesLoading = false, candidatesError = failure.asAppError(), error = failure.asAppError()) }
            return
        }
        _state.update { it.copy(candidatesLoading = false) }
        startFlow(final)
    }

    /** Creates the [SourceFlow] on [sources] (or shows "no sources") and starts it. */
    private fun startFlow(sources: List<RankedSource>) {
        _state.update { it.copy(sources = sources, canPickSource = sources.isNotEmpty()) }
        if (sources.isEmpty()) {
            val asked = extensions.installed.value.any { it.enabled }
            _state.update { it.copy(phase = PlayerPhase.NO_SOURCES, sourcesAsked = asked) }
            return
        }
        flow = SourceFlow(sources, settings.sourcePrefs.mode, pickConfirmed = args.sourceConfirmed && args.sourceKey != null)
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
                // 0.4.0: while the PC plays, the engine's state says the phase (the flow only waits behind it)
                phase = if (remoteId != null) it.phase else when (f.phase) {
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
                remaining = f.remaining(),
                mode = f.mode,
            )
        }
        refreshProcessing()
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
            SourceKind.TORRENT -> {
                // 0.4.0: starting on the PC: the PC streams the torrent from its own engine, the phone never downloads it
                // (a cached debrid link goes as a link: faster for the PC than the swarm)
                if (pendingStart != null && src.candidate.debrid?.cached != true && torrentLinkSource(src) != null) {
                    startOnPcFirst(null, torrentLinkSource(src), attempt)
                    return
                }
                if (!resolveDebrid(src, attempt)) resolveTorrent(src, attempt)
            }
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
        // 0.4.0: starting on the PC (Settings default / previous episode): the resolved stream goes to the PC instead
        if (pendingStart != null && engine != PlayerEngine.EXTERNAL) {
            startOnPcFirst(source, null, attempt)
            return
        }
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
        // 0.4.0: a Link session heartbeats; the flag is per engine session, and every switch of location makes a new one
        val s = factory.create(viewModelScope, listener(), remoteId != null)
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

    private fun loadIntoSession(source: PlayableSource, startOverride: Long? = null) {
        val s = ensureSession()
        val resumeAt = pendingResumeMs.also { pendingResumeMs = null }
        val resume = startOverride ?: resumeAt ?: s.currentPositionMs().takeIf { loadsIssued > 0 && it > 0 } ?: args.startMs
        loadsIssued++
        autoSkippedIntro = false
        currentSource = source
        if (remoteId == null) localSource = source
        customCues = null
        fetchedPrimaryFor = null
        pretranslatedFor = null
        _screenSubs.value = ScreenSubtitles()
        _state.update { it.copy(customSubtitle = null, externalSubtitles = source.subtitles, secondaryExternal = null) }
        s.load(source, resume)
    }

    private fun onSessionState(st: PlaybackState) {
        st.shaderNotice?.let { n ->
            if (n.seq != shaderNoticeSeq) {
                shaderNoticeSeq = n.seq
                _notices.trySend(PlayerNotice.ShaderChanged(n.kind))
            }
        }
        if (st.loadId != loadsIssued || loadsIssued == 0) return
        if (st.positionMs > 0) lastPositionMs = st.positionMs
        if (st.durationMs > 0) lastDurationMs = st.durationMs
        val f = flow
        val err = st.error
        if (err != null) {
            if (handledErrorLoad != st.loadId) {
                handledErrorLoad = st.loadId
                if (recoverMpvStall(err)) return
                if (rejectLinkProfile(err)) return
                if (recoverDecode(err)) return
                if (f != null && playbackInFlow()) {
                    if (playback != null) endTorrent(completed = false)
                    fail(f.attempt, FailKind.PLAYBACK, "${err.name} (${err.code})")
                } else if (remoteId != null) {
                    recoverRemote(auto = true)
                } else {
                    _state.update { it.copy(phase = PlayerPhase.ERROR, error = AppError.Unknown(err.name)) }
                }
            }
            // The failed load keeps publishing its (errored) state every tick: nothing below may run for it, or the
            // phase went back to PLAYING / BUFFERING and the error screen (and its Retry) disappeared, leaving a frozen player.
            return
        }
        if (st.started && handledStartLoad != st.loadId) {
            handledStartLoad = st.loadId
            // 0.4.0: a PC session's frames say nothing about the phone's source
            val key = if (remoteId == null) f?.started() else null
            if (key != null) viewModelScope.launch { runCatching { extensions.reportSourceResult(key, true) } }
        }
        restoreAudio(st)
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
            !nextDismissed && _state.value.canNavigateEpisodes
        ) {
            startNextCountdown()
        }
    }

    /**
     * BUG-020 / BUG-042 (plan §11.2): a Media3 decoder error first gets one silent reload at the same position when the device decodes the
     * video in hardware (transient errors, e.g. right after a seek); a video without a hardware decoder (HEVC 10 bits on the moto g51), or
     * a second failure, moves to libmpv (FFmpeg software decoding) with a discreet notice. Returns true when the error was handled here.
     */
    private fun recoverDecode(err: PlaybackError): Boolean {
        val src = currentSource ?: return false
        val inFlow = flow != null && playbackInFlow()
        val key = if (inFlow) "attempt:${flow?.attempt}" else "url:${src.url}"
        if (key != decodeRetryKey) {
            decodeRetryKey = key
            decodeRetries = 0
        }
        val tags = if (inFlow) flow?.currentSource()?.candidate?.tags else null
        val hardware = tags?.let { decodesInHardware(it, DeviceCodecs.capabilities()) } ?: true
        val mpvOk = mpvFactory != null && remoteId == null
        val position = session?.currentPositionMs()?.takeIf { it > 0 } ?: lastPositionMs.takeIf { it > 0 }
        return when (decodeRecovery(err.code, engine == PlayerEngine.MEDIA3, mpvOk, hardware, decodeRetries)) {
            DecodeRecovery.RETRY_SAME -> {
                decodeRetries++
                loadIntoSession(src, startOverride = position)
                true
            }
            DecodeRecovery.SWITCH_TO_MPV -> {
                switchToMpv(src, position)
                true
            }
            DecodeRecovery.NONE -> false
        }
    }

    /**
     * BUG-059: libmpv gave up on a stream that showed no frame in time (an HLS master whose renditions are not all ready, seen with the
     * PC's extension-server sources): the same source plays with Media3, which opens only the renditions it needs. Returns true when the
     * error was handled here.
     */
    private fun recoverMpvStall(err: PlaybackError): Boolean {
        val src = currentSource ?: return false
        if (!switchToMedia3OnStall(err.code, engine == PlayerEngine.MPV, remoteId != null, media3TriedFor == src.url)) return false
        media3TriedFor = src.url
        val position = session?.currentPositionMs()?.takeIf { it > 0 } ?: lastPositionMs.takeIf { it > 0 }
        switchEngine(PlayerEngine.MEDIA3, src, position, PlayerNotice.SwitchedToMedia3)
        return true
    }

    /** Replaces the Media3 session with a libmpv one and reloads [src] at [positionMs] (BUG-020). */
    private fun switchToMpv(src: PlayableSource, positionMs: Long?) {
        // Media3 already failed this source: an mpv open error (BUG-080) must not send it back to Media3.
        media3TriedFor = src.url
        switchEngine(PlayerEngine.MPV, src, positionMs, PlayerNotice.SwitchedToMpv)
    }

    /** Replaces the session with one of [target] for the rest of this player (the setting is not changed) and reloads [src]. */
    private fun switchEngine(target: PlayerEngine, src: PlayableSource, positionMs: Long?, notice: PlayerNotice) {
        engine = target
        if (remoteId == null) localEngine = target // 0.4.0: "PC -> Phone" comes back to the engine that plays this source
        _state.update { it.copy(engine = target) }
        _notices.trySend(notice)
        replaceSession(src, positionMs ?: args.startMs)
    }

    /** Releases the engine session and loads [src] at [positionMs] in a new one of [engine] (speed kept). */
    private fun replaceSession(src: PlayableSource, positionMs: Long) {
        val speed = session?.state?.value?.speed
        releaseSession()
        loadIntoSession(src, startOverride = positionMs)
        if (speed != null && speed != 1f) session?.setSpeed(speed)
    }

    /** Releases the engine session; the next load makes a new one (of [engine], remote or not). */
    private fun releaseSession() {
        sessionJob?.cancel()
        sessionJob = null
        session?.release()
        session = null
        // loadId is per session: the new one starts again from 1.
        loadsIssued = 0
        handledErrorLoad = 0
        handledStartLoad = 0
        secondaryDecidedLoad = -1
        shaderNoticeSeq = 0
        _state.update { it.copy(session = null) }
    }

    /**
     * The current load belongs to the source flow (not a direct torrent / local / remote load). 0.4.0: never while the PC
     * plays (a flow stays behind a "Phone -> PC" switch, but the PC's errors go through the Link recovery, not the failover).
     */
    private fun playbackInFlow(): Boolean =
        remoteId == null && flow?.phase.let { it == FlowPhase.BUFFERING || it == FlowPhase.PLAYING || it == FlowPhase.RESOLVING }

    private fun listener() = object : PlaybackListener {
        override fun onProgress(positionMs: Long, durationMs: Long, force: Boolean) = recordProgress(positionMs, durationMs, force)

        override fun onHeartbeat(positionMs: Long, paused: Boolean) {
            val id = remoteId ?: return // read on every beat: a renewed session (BUG-038) takes over
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
        currentSource = source // BUG-046: "Play with Media3" reloads it
        _state.update { it.copy(external = ExternalUi(launch)) }
    }

    /** The screen started the external activity ([ok] = false when no app could take it). */
    fun onExternalLaunched(ok: Boolean) {
        _state.update { st -> st.copy(external = st.external?.copy(launched = true, failed = !ok)) }
    }

    /**
     * BUG-046: "Play with Media3" on the "no external player" screen: this session plays in the built-in Media3 engine instead (the
     * setting is not changed).
     */
    fun useBuiltInPlayer() {
        if (engine != PlayerEngine.EXTERNAL) return
        val source = currentSource ?: return
        engine = PlayerEngine.MEDIA3
        localEngine = PlayerEngine.MEDIA3
        _state.update { it.copy(engine = PlayerEngine.MEDIA3, external = null) }
        loadIntoSession(source)
        if (flow != null) publishFlow() else _state.update { it.copy(phase = PlayerPhase.BUFFERING) }
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
        _state.update { it.copy(metered = null, noFileTorrentId = null, error = null, rememberedPick = key) }
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

    /**
     * BUG-038: a Link session failed (or Retry on it). The PC is asked whether the session still exists: gone + started by this
     * phone -> a NEW session with the same source and profile at the current position; gone + sent by the PC -> "send it again";
     * alive -> auto: show the error; Retry: a new session too (a stuck one fails again when reloaded); PC unreachable -> "connection lost" (resumes when the network returns).
     */
    private fun recoverRemote(auto: Boolean) {
        val id = remoteId ?: return
        if (remoteRecovery?.isActive == true) return
        // BUG-131: the PC closed the old session for a profile change that is still waiting for the new one: that change
        // replaces the session (a recovery here would open a third one with the old profile)
        if (auto && profileChange.changing.value) return
        remoteRecovery = viewModelScope.launch {
            // BUG-138: nothing played yet (a hand-off renewed before its first load): the point the PC sent
            val positionMs = session?.currentPositionMs()?.takeIf { it > 0 } ?: lastPositionMs.takeIf { it > 0 } ?: args.startMs
            val alive = remote.sessionAlive(id)
            // BUG-009 / BUG-023 / BUG-058: Retry on a session the PC still has opens a NEW one at the same point (reloading
            // the stuck one failed again with `ParserException`); the automatic path still shows the error first.
            val renew = shouldRenewRemote(alive, remote.canRenew(id), auto)
            when {
                renew -> {
                    _state.update { it.copy(phase = PlayerPhase.BUFFERING, error = null, remoteFailure = null) }
                    remote.renew(id, positionMs / 1000.0, retryLinkProfile()).fold(
                        onSuccess = { renewed ->
                            remoteId = renewed.sessionId
                            val src = remoteSourceFor(location, renewed.sessionId)
                            if (src == null) {
                                _state.update { it.copy(phase = PlayerPhase.REMOTE_UNAVAILABLE) }
                            } else {
                                // "the session had expired" only when it had; a Retry on a stuck one just plays again
                                if (alive == false) _notices.trySend(PlayerNotice.LinkSessionRenewed)
                                loadRenewed(src, positionMs)
                            }
                        },
                        onFailure = { e ->
                            // an older PC (no `session` source) or a session it forgot: only a new send helps
                            val api = (e.linkErrorOrNull() as? LinkError.Api)?.error
                            val ended = api == LinkApiError.RequestInvalid || api == LinkApiError.SessionNotFound || api == LinkApiError.SessionGone
                            remoteError(if (ended) RemoteFailure.SESSION_ENDED else RemoteFailure.PC_ERROR, e.asAppError())
                        },
                    )
                }
                alive == false -> remoteError(RemoteFailure.SESSION_ENDED, null)
                alive == true -> if (auto) remoteError(RemoteFailure.PC_ERROR, null) else reloadRemote(id)
                else -> if (auto) remoteError(RemoteFailure.CONNECTION_LOST, AppError.Offline) else reloadRemote(id)
            }
        }
    }

    /** BUG-022: the profiles the PC offers, for the in-player profile sheet (best effort: no sheet without them). */
    private fun loadLinkProfiles(sessionId: String) {
        viewModelScope.launch {
            val caps = loadCapabilities() ?: return@launch
            val current = remote.profileOf(sessionId) ?: caps.defaultProfile
            _state.update { it.copy(linkProfile = it.linkProfile ?: current) }
            if (location == ProcessingLocation.PC && pcProfile == null) pcProfile = _state.value.linkProfile
            refreshProcessing()
            // BUG-132: the session started with the closest playable profile instead of the saved preferred one: say it once
            link.takeProfileFallback()?.let { f ->
                val from = caps.profiles.firstOrNull { it.id == f.from }
                val to = caps.profiles.firstOrNull { it.id == f.to }
                if (from != null && to != null) _notices.trySend(PlayerNotice.LinkPreferredReplaced(from, to))
            }
        }
    }

    /**
     * BUG-132: a Link session whose stream this phone's decoder does not take (the moto g51 and 2560x1440): the profile is
     * remembered as rejected (disabled in the sheet, never asked again, Retry included) and the session goes back to the profile
     * used before the change, or to the closest playable one, with a notice that names the phone. Without one, the error screen
     * says the phone cannot play it. Returns true when the error was handled here.
     */
    private fun rejectLinkProfile(err: PlaybackError): Boolean {
        val id = remoteId ?: return false
        val bad = _state.value.linkProfile ?: remote.profileOf(id)
        val src = currentSource
        val retrySpent = src != null && decodeRetryKey == "url:${src.url}" && decodeRetries >= app.tatakai.next.player.MAX_SILENT_RETRIES
        if (!linkDecoderRejected(err.code, bad != null && bad in _state.value.linkUnsupported, retrySpent)) return false
        runCatching { android.util.Log.i(LINK_LOG_TAG, "phone decoder rejected session=$id profile=${bad ?: "-"} code=${err.code} (${err.name})") }
        if (bad != null) rejectedProfiles += bad
        val profiles = _state.value.linkProfiles
        _state.update { it.copy(linkUnsupported = unsupportedLinkProfiles(profiles, linkLimits, rejectedProfiles)) }
        refreshProcessing()
        val back = linkProfileAfterRejection(bad, profileBeforeChange, profiles, linkLimits, rejectedProfiles, linkDefaultProfile)
        if (back == null || bad == null) {
            remoteError(RemoteFailure.PHONE_DECODER, null)
            return true
        }
        val positionMs = session?.currentPositionMs()?.takeIf { it > 0 } ?: lastPositionMs
        _state.update { it.copy(phase = PlayerPhase.BUFFERING, error = null, remoteFailure = null) }
        profileChange.request(
            sessionId = id,
            currentProfile = bad,
            profileId = back,
            positionMs = positionMs,
            onStarted = { _state.update { it.copy(linkProfileChanging = true) } },
            onResult = { result ->
                _state.update { it.copy(linkProfileChanging = profileChange.changing.value) }
                if (result is LinkProfileResult.Changed && playProfileChange(result)) {
                    profileBeforeChange = null
                    _notices.trySend(PlayerNotice.LinkPhoneCannotPlay(profiles.firstOrNull { it.id == back }))
                } else {
                    remoteError(RemoteFailure.PHONE_DECODER, null)
                }
            },
        )
        return true
    }

    /**
     * BUG-022: restart the Link session with [profileId] at the current position (a new session from the same source; the
     * PC replaces the old one). The choice is not saved as the preferred profile (that stays on the Linked screen).
     */
    fun changeLinkProfile(profileId: String) {
        if (profileId in _state.value.linkUnsupported) return // BUG-132: a disabled row (this phone cannot play it)
        // 0.4.0: in Mixto the phone generates the frames: only upscale-only profiles (the list offers no other)
        if (location == ProcessingLocation.MIXED && capabilities?.profiles?.firstOrNull { it.id == profileId }?.frameGeneration == true) return
        val before = _state.value.linkProfile
        val positionMs = session?.currentPositionMs()?.takeIf { it > 0 } ?: lastPositionMs
        profileChange.request(
            sessionId = remoteId,
            currentProfile = _state.value.linkProfile,
            profileId = profileId,
            positionMs = positionMs,
            onStarted = { _state.update { it.copy(linkProfileChanging = true) }; refreshProcessing() },
            onResult = { result ->
                _state.update { it.copy(linkProfileChanging = profileChange.changing.value) }
                refreshProcessing()
                if (result is LinkProfileResult.Changed) {
                    profileBeforeChange = before
                    if (!playProfileChange(result)) {
                        _notices.trySend(PlayerNotice.LinkProfile(LinkProfileResult.Failed(LinkProfileFailure.PC_ERROR)))
                        return@request
                    }
                } else if (result is LinkProfileResult.Failed && session?.state?.value?.error != null) {
                    // the old session failed while the change ran (its recovery waited for it): recover it now
                    recoverRemote(auto = true)
                }
                _notices.trySend(PlayerNotice.LinkProfile(result))
            },
        )
    }

    /** BUG-131 / BUG-132: plays the new session of a profile change; false when its playlist cannot be opened (REMOTE_UNAVAILABLE). */
    private fun playProfileChange(result: LinkProfileResult.Changed): Boolean {
        remoteId = result.sessionId
        if (location == ProcessingLocation.PC) pcProfile = result.profileId
        _state.update { it.copy(linkProfile = result.profileId, phase = PlayerPhase.BUFFERING, error = null, remoteFailure = null) }
        refreshProcessing()
        val src = remoteSourceFor(location, result.sessionId)
        if (src == null) {
            _state.update { it.copy(phase = PlayerPhase.REMOTE_UNAVAILABLE) }
            return false
        }
        loadRenewed(src, result.positionMs)
        return true
    }

    /** BUG-132: the profile a Retry / recovery asks the PC for: never one this phone's decoder already rejected. */
    private fun retryLinkProfile(): String? {
        val current = _state.value.linkProfile
        val wanted = linkRetryProfile(current, _state.value.linkProfiles, linkLimits, rejectedProfiles, linkDefaultProfile)
        if (wanted != current) _state.update { it.copy(linkProfile = wanted) }
        return wanted
    }

    /**
     * BUG-131: a renewed Link session (other profile, or recovered) plays in a NEW engine session: reusing the old one kept its
     * decoders and audio sink (r6: after an audio timestamp discontinuity of the old stream the reused decoder never got input
     * again and the player stayed "preparing" with 60 s buffered). If it shows no frame in [RENEWED_START_TIMEOUT_MS] it is
     * opened once more through the Retry path (a new PC session at the same point), then the error screen with Retry.
     */
    private fun loadRenewed(src: PlayableSource, positionMs: Long) {
        replaceSession(src, positionMs)
        watchRenewedStart(positionMs)
    }

    private fun watchRenewedStart(positionMs: Long, retried: Boolean = false) {
        renewWatch?.cancel()
        val load = loadsIssued
        renewWatch = viewModelScope.launch {
            delay(RENEWED_START_TIMEOUT_MS)
            if (load != loadsIssued || handledStartLoad == load || handledErrorLoad == load) return@launch
            val id = remoteId ?: return@launch
            runCatching { android.util.Log.i(LINK_LOG_TAG, "renewed session $id did not start in ${RENEWED_START_TIMEOUT_MS}ms (retried=$retried)") }
            if (retried) {
                remoteError(RemoteFailure.PC_ERROR, null)
                return@launch
            }
            val renewed = remote.renew(id, positionMs / 1000.0, retryLinkProfile()).getOrNull()
            val src = renewed?.let { remoteSourceFor(location, it.sessionId) }
            if (renewed == null || src == null) {
                remoteError(RemoteFailure.PC_ERROR, null)
                return@launch
            }
            remoteId = renewed.sessionId
            _notices.trySend(PlayerNotice.LinkProfileRetried)
            replaceSession(src, positionMs)
            watchRenewedStart(positionMs, retried = true)
        }
    }

    private fun remoteError(kind: RemoteFailure, error: AppError?) {
        _state.update { it.copy(phase = PlayerPhase.ERROR, error = error, remoteFailure = kind) }
    }

    fun retry() {
        val f = flow
        _state.update { it.copy(error = null, noFileTorrentId = null, remoteFailure = null) }
        // 0.4.0: a PC session started from the player keeps its source flow behind it: Retry is the Link one
        if (remoteId != null) {
            recoverRemote(auto = false)
            return
        }
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
            // 0.4.0: from the PC the next episode is resolved on the phone and sent again (ProcessingCarry), never this session
            remoteSessionId = null,
            sourceKey = null,
            torrentId = if (p != null && p.batch) p.torrentId else null,
            fileIndex = null,
            localUri = null,
        )
        ProcessingCarry.put(titleKey, location)
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
        if (episode < 1 || episode == args.episode || !_state.value.canNavigateEpisodes) return
        nextJob?.cancel()
        _state.update { it.copy(next = null) }
        ProcessingCarry.put(titleKey, location)
        _replace.trySend(
            args.copy(episode = episode, startMs = 0, remoteSessionId = null, sourceKey = null, torrentId = null, fileIndex = null, localUri = null),
        )
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

    // ---- 0.4.0 (worker C): Procesar en Teléfono | PC | Mixto ----------------------------------------------------------------------

    /**
     * Worker D's "Procesar en" control: switches to [to] keeping the position (and the audio stream). The video keeps playing
     * where it is until the other side is ready; a failure leaves it there with a notice ([PlayerNotice.Processing]). A blocked
     * option only says why ([ProcessingOption.block]). Choosing the current location while a switch runs cancels it.
     */
    fun setProcessingLocation(to: ProcessingLocation) {
        if (switcher.switching.value != null) {
            if (to == location) switcher.cancel()
            return
        }
        if (to == location) return
        val option = _processing.value.option(to)
        if (!option.available) {
            _notices.trySend(PlayerNotice.Processing(to, block = option.block, detail = option.detail))
            return
        }
        pendingStart = null
        viewModelScope.launch {
            val profile = profileFor(to)
            // the capabilities may have arrived just now (client_fg, the upscale profiles): ask again
            val now = _processing.value.option(to)
            if (!now.available || (to == ProcessingLocation.MIXED && profile == null)) {
                _notices.trySend(PlayerNotice.Processing(to, block = now.block ?: ProcessingBlock.PC_NO_UPSCALE_PROFILE, detail = now.detail))
                return@launch
            }
            switcher.request(location, to, remoteId, profile) { result -> onSwitchResult(result) }
        }
    }

    /** Worker D's profile list ([ProcessingUiState.profileGroups]): the PC restarts the session with [profileId] at the same point. */
    fun setLinkProfile(profileId: String) = changeLinkProfile(profileId)

    /** Cancels a running "Procesar en" switch (the video goes on where it plays). */
    fun cancelProcessingSwitch() = switcher.cancel()

    private fun onSwitchResult(result: ProcessingSwitchResult) {
        when (result) {
            is ProcessingSwitchResult.Switched -> _notices.trySend(PlayerNotice.Processing(result.to))
            is ProcessingSwitchResult.Failed -> _notices.trySend(PlayerNotice.Processing(result.to, failure = result.reason))
            ProcessingSwitchResult.Same, ProcessingSwitchResult.Busy, ProcessingSwitchResult.Cancelled -> Unit
        }
        refreshProcessing()
    }

    /**
     * The profile [to] asks the PC for: PC = the one used before in PC (Mixto keeps its own, so going back brings it back), from
     * the phone null = the preferred one of the Linked screen; Mixto = an upscale-only one ([mixedLinkProfile]). null for Mixto
     * = none available.
     */
    private suspend fun profileFor(to: ProcessingLocation): String? = when (to) {
        ProcessingLocation.PHONE -> null
        ProcessingLocation.PC -> pcProfile?.takeIf { it !in rejectedProfiles } ?: if (location == ProcessingLocation.MIXED) {
            loadCapabilities()?.let { c ->
                playableLinkProfile(link.linkedDevice.value?.preferredProfile, c.profiles, linkLimits, rejectedProfiles, c.defaultProfile)
            }
        } else {
            null
        }
        ProcessingLocation.MIXED -> loadCapabilities()?.let { c ->
            mixedLinkProfile(_state.value.linkProfile ?: pcProfile ?: link.linkedDevice.value?.preferredProfile, c.profiles, linkLimits, rejectedProfiles)
        }
    }

    /**
     * [pendingStart]: the first resolved source of this player goes to the PC instead of the phone ([source] = a stream the phone
     * resolved, [linkSource] = a torrent the PC streams itself). When that location is blocked or the PC cannot take it, the
     * phone plays it, with a notice.
     */
    private fun startOnPcFirst(source: PlayableSource?, linkSource: LinkSource?, attempt: Long) {
        val want = pendingStart ?: return
        pendingStart = null
        val f = flow
        if (source != null) localSource = source
        pendingLinkSource = linkSource
        _state.update { it.copy(phase = PlayerPhase.BUFFERING) }
        refreshProcessing()
        fun playHere() {
            pendingLinkSource = null
            if (f != null && attempt != f.attempt) return
            if (source != null) {
                f?.resolved(attempt)
                loadIntoSession(source)
                publishFlow()
            } else {
                val src = f?.currentSource() ?: return
                resolveJob?.cancel()
                resolveJob = viewModelScope.launch { if (!resolveDebrid(src, attempt)) resolveTorrent(src, attempt) }
            }
            refreshProcessing()
        }
        val option = _processing.value.option(want)
        if (!option.available) {
            _notices.trySend(PlayerNotice.Processing(want, block = option.block, detail = option.detail, onPhoneInstead = true))
            playHere()
            return
        }
        viewModelScope.launch {
            val profile = profileFor(want)
            val now = _processing.value.option(want)
            if (!now.available || (want == ProcessingLocation.MIXED && profile == null)) {
                val block = now.block ?: ProcessingBlock.PC_NO_UPSCALE_PROFILE
                _notices.trySend(PlayerNotice.Processing(want, block = block, detail = now.detail, onPhoneInstead = true))
                playHere()
                return@launch
            }
            switcher.request(ProcessingLocation.PHONE, want, null, profile) { result ->
                when (result) {
                    is ProcessingSwitchResult.Switched -> {
                        f?.resolved(attempt)
                        _notices.trySend(PlayerNotice.Processing(want))
                    }
                    is ProcessingSwitchResult.Failed -> {
                        _notices.trySend(PlayerNotice.Processing(want, failure = result.reason, onPhoneInstead = true))
                        playHere()
                    }
                    else -> playHere()
                }
                refreshProcessing()
            }
        }
    }

    /**
     * Loads the Link session [sessionId] for [to] (PC: Media3 with the mTLS client; Mixto: libmpv through the loopback proxy)
     * at the current position, after the phone's own playback (its torrent stops: the PC streams it itself). The phone's
     * shaders go off (worker A) and its frame generation follows [to] (worker B) before the load.
     */
    private fun playOnPc(to: ProcessingLocation, sessionId: String, profileId: String?): Boolean {
        val src = remoteSourceFor(to, sessionId) ?: return false
        val positionMs = positionNow()
        if (location == ProcessingLocation.PHONE) {
            session?.state?.value?.audioTracks?.firstOrNull { it.selected }?.let { pendingAudio = it.language to it.label }
            if (playback != null) endTorrent(completed = false)
        }
        pendingLinkSource = null
        renewWatch?.cancel()
        remoteId = sessionId
        location = to
        lastStatus = null
        val profile = profileId ?: remote.profileOf(sessionId) ?: capabilities?.defaultProfile
        if (to == ProcessingLocation.PC) pcProfile = profile
        engine = engineFor(to, localEngine, mpvFactory != null)
        _state.update {
            it.copy(
                engine = engine, remoteless = false, linkProfile = profile, phase = PlayerPhase.BUFFERING, error = null, remoteFailure = null,
                torrent = null, metered = null,
            )
        }
        applyGates()
        loadRenewed(src, positionMs)
        loadLinkProfiles(sessionId)
        startStatusPolling()
        refreshProcessing()
        return true
    }

    /**
     * "PC -> Phone": the original source plays again on the phone at [positionMs]: the web stream it played (or sent) when there
     * is one, else the source flow's current source resolved again (a torrent), else the title's normal source flow (a PC
     * hand-off). false = the phone has nothing to play.
     */
    private fun playOnPhone(positionMs: Long): Boolean {
        if (!hasOriginal()) return false
        statusJob?.cancel()
        renewWatch?.cancel()
        remoteId = null
        lastStatus = null
        location = ProcessingLocation.PHONE
        engine = localEngine
        _state.update {
            it.copy(engine = engine, remoteless = true, linkProfileChanging = false, remoteFailure = null, error = null, phase = PlayerPhase.BUFFERING)
        }
        applyGates()
        val saved = localSource
        val f = flow
        val current = f?.currentSource()
        when {
            saved != null && !saved.torrent -> {
                replaceSession(saved, positionMs)
                if (f != null) publishFlow()
            }
            f != null && current != null -> {
                releaseSession()
                pendingResumeMs = positionMs
                if (f.pick(current.key)) advance()
            }
            else -> {
                releaseSession()
                pendingResumeMs = positionMs
                viewModelScope.launch {
                    val m = media ?: runCatching { metadata.media(MediaKind.ANIME, args.media) }.getOrNull()?.also { media = it }
                    val torrentId = args.torrentId
                    when {
                        m == null -> _state.update { it.copy(phase = PlayerPhase.FAILED, error = AppError.Unknown("media")) }
                        torrentId != null -> playTorrentDirect(m, torrentId, args.fileIndex)
                        else -> loadCandidates(m)
                    }
                }
            }
        }
        refreshProcessing()
        return true
    }

    /** The phone can play this title itself (a source it played or can resolve, or a title it can look up). */
    private fun hasOriginal(): Boolean = flow != null || localSource != null || args.torrentId != null || titleKnown

    /** The Link session's HLS for [loc]'s engine: the loopback proxy for libmpv, the mTLS client for Media3. */
    private fun remoteSourceFor(loc: ProcessingLocation, sessionId: String): PlayableSource? {
        if (engineFor(loc, localEngine, mpvFactory != null) == PlayerEngine.MPV) {
            val url = remote.loopbackUrl(sessionId) ?: return null
            // the PC's stream carries video + one audio stream only (tk-link maps no subtitles): libmpv side-loads the phone
            // source's external tracks itself (Media3 cannot: its client for the PC trusts only the PC's CA)
            val subtitles = localSource?.takeIf { !it.torrent }?.subtitles.orEmpty()
            return PlayableSource(url = url, headers = emptyMap(), container = Container.HLS, label = args.title, subtitles = subtitles)
        }
        return RemotePlayback.source(remote, sessionId, args.title)
    }

    /** Opens the playing Link session again (Retry on a session the PC still has, or when it could not be asked). */
    private fun reloadRemote(sessionId: String) {
        val src = remoteSourceFor(location, sessionId)
        if (src == null) _state.update { it.copy(phase = PlayerPhase.REMOTE_UNAVAILABLE) } else directLoad(src)
    }

    /** What the current phone playback can give the PC (for [ProcessingBlock]). */
    private fun sendableState(): SendableState {
        if (args.localUri != null) return SendableState.LocalFile
        if (pendingLinkSource != null) return SendableState.Sendable
        val src = localSource ?: return SendableState.NothingYet
        if (!src.torrent) return if (src.url.startsWith("http://") || src.url.startsWith("https://")) SendableState.Sendable else SendableState.NotSendable
        return if (args.media.anilistId != null && torrentUriNow() != null) SendableState.Sendable else SendableState.NotSendable
    }

    /** The current phone playback as a Link source (a web / debrid stream as a link, a torrent as the torrent itself). */
    private fun currentLinkSourceOrNull(): LinkSource? {
        pendingLinkSource?.let { return it }
        val src = localSource ?: return null
        if (!src.torrent) return src.takeIf { it.url.startsWith("http://") || it.url.startsWith("https://") }?.let { LinkSource.Url(it.url, it.headers) }
        // the phone's torrent stream is its own loopback server: the PC gets the torrent (it needs the AniList id for the file)
        val id = args.media.anilistId ?: return null
        val uri = torrentUriNow() ?: return null
        return LinkSource.Torrent(uri, id.toString(), args.episode)
    }

    private fun torrentUriNow(): String? =
        flow?.currentSource()?.candidate?.takeIf { it.kind == SourceKind.TORRENT }?.torrentUri() ?: playback?.torrentId ?: args.torrentId

    private fun torrentLinkSource(src: RankedSource): LinkSource? {
        val id = args.media.anilistId ?: return null
        val uri = src.candidate.torrentUri() ?: return null
        return LinkSource.Torrent(uri, id.toString(), args.episode)
    }

    /** The PC's capabilities (profiles, `client_fg`), asked once per player; null while the PC cannot be asked. */
    private suspend fun loadCapabilities(): LinkCapabilities? {
        capabilities?.let { return it }
        val caps = link.capabilities().getOrNull() ?: return null
        capabilities = caps
        linkDefaultProfile = caps.defaultProfile
        linkLimits = runCatching { linkDecodeLimits() }.getOrNull()
        _state.update {
            it.copy(
                linkProfiles = caps.profiles,
                linkUnsupported = unsupportedLinkProfiles(caps.profiles, linkLimits, rejectedProfiles),
                linkMayDropFrames = linkProfilesMayDropFrames(caps.profiles, linkLimits),
            )
        }
        refreshProcessing()
        return caps
    }

    /** `GET /v1/sessions/{id}` every [STATUS_POLL_MS] while the PC plays: what it does, for [ProcessingUiState.pc]. */
    private fun startStatusPolling() {
        statusJob?.cancel()
        statusJob = viewModelScope.launch {
            while (isActive) {
                val id = remoteId ?: break
                val status = runCatching { remote.sessionStatus(id) }.getOrNull()
                if (status != null && id == remoteId) {
                    lastStatus = status
                    refreshProcessing()
                }
                delay(STATUS_POLL_MS)
            }
        }
    }

    /** Worker A / B: the phone's shaders off while the PC processes; its frame generation per location. */
    private fun applyGates() {
        runCatching { enhancementGate.setRemoteProcessing(location != ProcessingLocation.PHONE) }
        runCatching { frameGenGate.setProcessingLocation(location) }
    }

    /** "PC -> Phone": the audio language the phone played before going to the PC is selected again (once per load). */
    private fun restoreAudio(st: PlaybackState) {
        val want = pendingAudio ?: return
        if (remoteId != null || st.audioTracks.isEmpty() || audioRestoredLoad == st.loadId) return
        audioRestoredLoad = st.loadId
        pendingAudio = null
        if (st.audioTracks.size < 2) return
        val (language, label) = want
        val match = st.audioTracks.firstOrNull { language != null && it.language == language }
            ?: st.audioTracks.firstOrNull { label != null && it.label == label }
            ?: return
        if (!match.selected) session?.selectAudio(match.id)
    }

    private fun positionNow(): Long = session?.currentPositionMs()?.takeIf { it > 0 } ?: lastPositionMs.takeIf { it > 0 } ?: args.startMs

    /** [ProcessingCarry] key: the title (history id, else its name). */
    private val titleKey: String get() = mediaId ?: args.title

    /** Recomputes [processing] from the player's facts (cheap; called on every change that matters). */
    private fun refreshProcessing() {
        val caps = capabilities
        val linkState = link.state.value
        val paired = linkState is LinkState.Paired
        val reachable = (linkState as? LinkState.Paired)?.reachable == true
        val frameGen = frameGenGate.availability.value
        val st = _state.value
        val current = st.linkProfile
        val mixed = caps?.let { mixedLinkProfile(current ?: pcProfile ?: link.linkedDevice.value?.preferredProfile, it.profiles, linkLimits, rejectedProfiles) }
        val facts = ProcessingFacts(
            current = location,
            paired = paired,
            reachable = reachable,
            external = engine == PlayerEngine.EXTERNAL,
            sendable = sendableState(),
            hasOriginal = hasOriginal(),
            mpvAvailable = mpvFactory != null,
            phoneFrameGen = frameGen,
            clientFgAllowed = caps?.clientFrameGenerationAllowed,
            capabilitiesKnown = caps != null,
            mixedProfile = mixed,
        )
        val pcName = (linkState as? LinkState.Paired)?.pc?.serverName ?: link.linkedDevice.value?.serverName
        val remoteNow = location != ProcessingLocation.PHONE && remoteId != null
        val profileId = lastStatus?.profileId ?: current ?: caps?.defaultProfile
        val pc = if (remoteNow && profileId != null) {
            linkProcessingInfo(pcName, profileId, caps?.profiles?.firstOrNull { it.id == profileId }, lastStatus)
        } else {
            null
        }
        val groups = caps?.let { groupedLinkProfiles(it.profiles, linkLimits, rejectedProfiles, current, upscaleOnly = location == ProcessingLocation.MIXED) }
        _processing.update {
            it.copy(
                location = location,
                options = processingOptions(facts),
                switchingTo = switcher.switching.value,
                pc = pc,
                pcName = pcName,
                profileGroups = groups.orEmpty(),
                profile = current.takeIf { remoteNow },
                profileChanging = st.linkProfileChanging,
                phoneFrameGen = location == ProcessingLocation.MIXED,
                phoneFrameGenAvailability = frameGen,
                localEnhancementsOff = location != ProcessingLocation.PHONE,
            )
        }
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
        switcher.cancel()
        statusJob?.cancel()
        session?.release() // emits the final forced progress tick
        session = null
        val completed = lastDurationMs > 0 && lastPositionMs >= lastDurationMs * COMPLETED_FRACTION
        endTorrent(completed)
        remoteId?.let { id -> detached.launch { link.stop(id) } }
        // 0.4.0: the phone's own enhancements are the user's again once nothing plays from the PC
        if (location != ProcessingLocation.PHONE) {
            runCatching { enhancementGate.setRemoteProcessing(false) }
            runCatching { frameGenGate.setProcessingLocation(ProcessingLocation.PHONE) }
        }
    }

    private fun Throwable.asAppError(): AppError = (this as? AppException)?.error ?: AppError.Unknown(message ?: javaClass.simpleName)

    private fun noFileTorrent(e: AppError): String? =
        (e as? AppError.Unknown)?.msg?.takeIf { it.startsWith(NO_FILE_PREFIX) }?.removePrefix(NO_FILE_PREFIX)?.ifBlank { null }

    private companion object {
        /** BUG-019: Auto mode starts with the sources that answered by then instead of waiting for the slowest extension. */
        const val AUTO_START_MS = 30_000L
        const val MIN_COUNTDOWN_SECS = 3
        const val MAX_COUNTDOWN_SECS = 30
        const val SEEK_MARKS_POLL_MS = 2_500L
        const val COMPLETED_FRACTION = 0.9
        const val NO_FILE_PREFIX = "torrent:no_file:"

        /** BUG-131: logcat tag of the Link profile changes (`adb logcat -s TatakaiLink`). */
        const val LINK_LOG_TAG = "TatakaiLink"

        /** BUG-131: a renewed Link session that shows no frame in this time is opened once more. */
        const val RENEWED_START_TIMEOUT_MS = 20_000L

        /** BUG-138: how long start() waits for the PC to say whether the session still exists (then it just loads it). */
        const val START_ALIVE_CHECK_TIMEOUT_MS = 2_000L

        /** 0.4.0: how often the "En la PC" indicator asks the PC what it does (`GET /v1/sessions/{id}`). */
        const val STATUS_POLL_MS = 5_000L
    }
}

/** Identity of a translation setup (the key itself is never kept, only whether / which one is set). */
private data class SubtitleTranslationConfigKey(val settings: app.tatakai.next.model.SubtitleTranslation, val keyHash: Int?)
