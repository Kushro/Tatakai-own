// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.AspectMode
import app.tatakai.next.model.DebandConfig
import app.tatakai.next.model.DecoderMode
import app.tatakai.next.model.ImageAdjust
import app.tatakai.next.model.QualityTier
import app.tatakai.next.model.SharpenLevel
import app.tatakai.next.model.SubtitleStyle
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

// android wave (docs/redesign/12 §4 d.1): engine abstraction of the player. Private to worker d's modules: :core:player implements it
// with Media3 ([PlaybackController]) and :core:player-mpv with libmpv (MpvPlayerSession). The EXTERNAL engine has no session (an Intent
// to another app, see ExternalPlayer.kt).

/**
 * One playback engine instance. Create and drive it on the main thread; [release] exactly once. [state] may be updated from any thread
 * (mpv reports from its event thread) and is safe to collect anywhere.
 */
interface PlayerSession {
    val state: StateFlow<PlaybackState>

    /** Replace whatever plays with [source], starting at [startMs]. Bumps [PlaybackState.loadId] and clears error / started. */
    fun load(source: PlayableSource, startMs: Long)

    fun play()

    fun pause()

    fun togglePlay()

    fun seekTo(positionMs: Long)

    fun seekBy(deltaMs: Long)

    fun setSpeed(speed: Float)

    /** Caps the video height; null = automatic. Engines without adaptive streams may ignore it. */
    fun selectVideoHeight(height: Int?)

    /** [SubtitleOption.groupIndex]; null turns subtitles off. */
    fun selectSubtitle(groupIndex: Int?)

    /** [AudioOption.id]. */
    fun selectAudio(id: Int)

    /** Preferred subtitle / audio languages (ISO 639-1, `SourcePrefs.subLang` / `dubLang`) for the next tracks the engine picks. */
    fun setPreferredLanguages(subtitle: String?, audio: String?)

    /** Settings › Reproductor › Estilo de subtítulos. ASS keeps its own styles where the engine renders them. */
    fun setSubtitleStyle(style: SubtitleStyle)

    /** Current position right now (the [state] tick may lag up to half a second). */
    fun currentPositionMs(): Long

    fun release()

    // ---- parity wave A (docs/redesign/15 §3.2, worker p): private engine extensions --------------------------------------------------

    /** Subtitle delay in ms (positive = later). mpv applies `sub-delay`; Media3 only records it (the screen shifts the cues it draws). */
    fun setSubtitleDelay(ms: Long) = Unit

    /** Colour / outline / position / custom font of text subtitles (mpv `sub-*`; Media3 draws them on screen from [SubtitleExtras]). */
    fun setSubtitleExtras(extras: SubtitleExtras) = Unit

    /**
     * Adds a local subtitle file ([path] on disk) and selects it. Returns false when the engine cannot (Media3: the screen draws a loaded
     * file itself from its parsed cues).
     */
    fun addSubtitleFile(path: String, title: String): Boolean = false

    /** True when [selectSecondarySubtitle] works on any track of the stream (mpv `secondary-sid`). */
    val supportsSecondaryTracks: Boolean get() = false

    /** Second subtitle track shown at the top; null = off. Returns false when unsupported (Media3 draws an external track itself). */
    fun selectSecondarySubtitle(id: Int?): Boolean = false

    /** Engine-drawn subtitles on / off (mpv `sub-visibility`), used while the screen draws a translated line instead. */
    fun setSubtitlesVisible(visible: Boolean) = Unit

    /** Replay the episode when it ends (Media3 `REPEAT_MODE_ONE`, mpv `loop-file`): no "ended", no next-episode card. */
    fun setLoop(loop: Boolean) = Unit

    /** Loudness normalisation ("stable volume"): Media3 `DynamicsProcessing` / `LoudnessEnhancer`, mpv `af=dynaudnorm`. */
    fun setStableVolume(enabled: Boolean) = Unit

    /** Writes the current frame as PNG to [path] (mpv `screenshot-to-file`). False when the engine cannot (Media3: `PixelCopy`). */
    fun screenshotToFile(path: String): Boolean = false

    // ---- 0.4.0 quality engine (worker A, specs/a-quality-engine.md). libmpv implements all of it live (no reload); the defaults are
    // no-ops so Media3 (and any other engine) reports [QualityUiState.MEDIA3] and the UI offers "Cambiar a MPV" instead. -----------

    /** Requested vs effective preset / tier, active shaders, reasons, decoder in use, drops, display Hz, renderer, last event. */
    val quality: StateFlow<QualityUiState> get() = QualityUiState.UNAVAILABLE

    /** Upscaling preset ([app.tatakai.next.model.QualityPresets] id) and tier, applied to the running video. */
    fun setQuality(preset: String, tier: QualityTier) = Unit

    /** Every knob at once (e.g. "reset to my defaults"). */
    fun setQualityRequest(request: QualityRequest) = Unit

    /** FidelityFX CAS sharpening on top of the preset ([SharpenLevel.OFF] = none). */
    fun setSharpen(level: SharpenLevel) = Unit

    /** Brightness / contrast / saturation / gamma / hue (-100..100). */
    fun setImage(image: ImageAdjust) = Unit

    /** Fit / fill (crop) / stretch / forced 16:9 or 4:3, plus zoom (25..400 %). */
    fun setAspect(mode: AspectMode, zoomPercent: Int = 100) = Unit

    /** Auto / HW (copy) / HW+ (direct) / SW; mpv re-opens the decoder in place. */
    fun setDecoder(mode: DecoderMode) = Unit

    fun setDeband(deband: DebandConfig) = Unit

    /** mpv interpolation (`display-resample` + `tscale=oversample`): smoother pacing, not real frame generation. */
    fun setSmoothing(enabled: Boolean) = Unit

    /** Thermal / battery / frame-drop adjustments for this playback. */
    fun setAutoAdjust(enabled: Boolean) = Unit

    /** Reverts the frame-drop step-down ([QualityUiState.autoDowngrade]) and stops stepping down again for this video. */
    fun undoAutoDowngrade() = Unit

    /** The player entered / left picture-in-picture (caps the tier). */
    fun setPictureInPicture(active: Boolean) = Unit

    /** Tatakai Link (worker C): the PC processes the video, so local shaders / deband / smoothing stay off while true. */
    fun setRemoteProcessing(active: Boolean) = Unit

    /** Phone frame generation (worker B) is presenting: mpv's smoothing is forced off while true. */
    fun setFrameGenActive(active: Boolean) = Unit
}

/**
 * Progress / heartbeat ticks shared by the engines: [PlaybackListener.onProgress] every [PLAYER_PROGRESS_INTERVAL_MS] while
 * playing, and [PlaybackListener.onHeartbeat] every [PLAYER_HEARTBEAT_INTERVAL_MS] for remote sessions. Forced ticks on pause /
 * release are the engine's job.
 */
class SessionTicker(
    private val scope: CoroutineScope,
    private val listener: PlaybackListener,
    private val remote: Boolean,
    private val now: () -> Long,
    private val snapshot: () -> Triple<Long, Long, Boolean>,
    private val onTick: () -> Unit = {},
) {
    private var job: Job? = null
    private var lastProgressAt = 0L
    private var lastHeartbeatAt = 0L

    fun start() {
        if (job != null) return
        job = scope.launch {
            while (true) {
                delay(PLAYER_TICK_MS)
                onTick()
                val (pos, dur, playing) = snapshot()
                val t = now()
                if (playing && t - lastProgressAt >= PLAYER_PROGRESS_INTERVAL_MS) {
                    lastProgressAt = t
                    listener.onProgress(pos, dur, false)
                }
                if (remote && t - lastHeartbeatAt >= PLAYER_HEARTBEAT_INTERVAL_MS) {
                    lastHeartbeatAt = t
                    listener.onHeartbeat(pos, !playing)
                }
            }
        }
    }

    fun stop() {
        job?.cancel()
        job = null
    }
}

/** Engine state refresh tick. */
const val PLAYER_TICK_MS: Long = 500L

/** History progress row while playing. */
const val PLAYER_PROGRESS_INTERVAL_MS: Long = 15_000L

/** Link (remote) heartbeat. */
const val PLAYER_HEARTBEAT_INTERVAL_MS: Long = 4_000L
