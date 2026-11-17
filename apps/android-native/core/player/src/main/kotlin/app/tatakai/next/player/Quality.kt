// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import androidx.annotation.StringRes
import app.tatakai.next.model.AspectMode
import app.tatakai.next.model.DebandConfig
import app.tatakai.next.model.DecoderMode
import app.tatakai.next.model.ImageAdjust
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.PlayerQualityPrefs
import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.model.SharpenLevel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

// 0.4.0 quality engine (worker A, artifacts/player-quality-0.4.0-specs/a-quality-engine.md): what the in-player UI (worker D) renders
// and asks for, for both engines. libmpv implements it (`MpvQualityController` in :core:player-mpv); Media3 publishes
// [QualityUiState.MEDIA3] (nothing available, "Cambiar a MPV" instead). Pure data: unit tested on the JVM.

/** Everything the user asked for in this playback (Settings › Reproductor gives the starting values, the player changes them live). */
data class QualityRequest(
    /** A [QualityPresets] id. */
    val preset: String = QualityPresets.OFF,
    val tier: QualityTier = QualityTier.BALANCED,
    val sharpen: SharpenLevel = SharpenLevel.OFF,
    val deband: DebandConfig = DebandConfig(),
    val image: ImageAdjust = ImageAdjust(),
    val aspect: AspectMode = AspectMode.FIT,
    /** 25..400, 100 = no zoom (mpv `video-zoom` is log2 of the factor). */
    val zoomPercent: Int = 100,
    val decoder: DecoderMode = DecoderMode.AUTO,
    /** mpv interpolation (frame pacing, NOT real frame generation). */
    val smoothing: Boolean = false,
    /** Thermal / battery / frame-drop adjustments ([PlayerQualityPrefs.autoAdjust]). */
    val autoAdjust: Boolean = true,
) {
    fun sanitized(): QualityRequest = copy(
        preset = QualityPresets.sanitize(preset),
        deband = deband.sanitized(),
        image = image.sanitized(),
        zoomPercent = zoomPercent.coerceIn(MIN_ZOOM, MAX_ZOOM),
    )

    companion object {
        const val MIN_ZOOM = 25
        const val MAX_ZOOM = 400

        /** The starting request of a playback from Settings › Reproductor ([preset] = `AppSettings.playerPreset`). */
        fun from(preset: String, prefs: PlayerQualityPrefs): QualityRequest = QualityRequest(
            preset = QualityPresets.sanitize(preset),
            tier = prefs.tier,
            sharpen = prefs.sharpen,
            deband = prefs.deband.sanitized(),
            decoder = prefs.decoder,
            smoothing = prefs.smoothing,
            autoAdjust = prefs.autoAdjust,
        ).sanitized()
    }
}

/** A preset at a tier. [tier] is null for presets without tiers (FSR, CAS) and when the preset is [QualityPresets.OFF]. */
data class QualityLevel(val preset: String, val tier: QualityTier?) {
    val off: Boolean get() = preset == QualityPresets.OFF

    companion object {
        val OFF = QualityLevel(QualityPresets.OFF, null)

        fun of(preset: String, tier: QualityTier?): QualityLevel =
            if (preset == QualityPresets.OFF) OFF else QualityLevel(preset, tier?.takeIf { preset in QualityPresets.TIERED })
    }
}

/**
 * Why the effective quality differs from the requested one (or why something is off), most important first in
 * [QualityUiState.reasons]. [labelRes] / [hintRes] are the short es/en texts for the UI and the notices.
 */
enum class QualityReason(@StringRes val labelRes: Int) {
    /** ArtCNN needs compute shaders on gpu-next + Vulkan; the bundled libmpv only has OpenGL ES. */
    RENDERER_UNSUPPORTED(R.string.quality_reason_renderer),

    /** A shader of the chain failed to compile on this GPU: the chain is off for this video. */
    SHADER_ERROR(R.string.quality_reason_shader_error),

    /** 8K source: no shaders, no deband. */
    RES_8K(R.string.quality_reason_8k),

    /** 4K source: upscaling a 4K video on a phone screen does nothing useful and costs a lot. */
    RES_4K(R.string.quality_reason_4k),

    /** Tatakai Link: the PC processes the video, the phone shows it as it comes. */
    REMOTE_PROCESSING(R.string.quality_reason_remote),

    /** Phone frame generation is presenting: mpv's smoothing is off (they would fight over the same frames). */
    FRAME_GEN(R.string.quality_reason_frame_gen),

    /** Battery <= 20 % and not charging: shaders, deband and smoothing off. */
    BATTERY_LOW(R.string.quality_reason_battery),

    /** The phone is hot: one tier lower (never off by heat alone). */
    THERMAL(R.string.quality_reason_thermal),

    /** Too many dropped frames: stepped down (undoable). */
    FRAME_DROPS(R.string.quality_reason_drops),

    /** Picture-in-picture: capped at the fastest tier. */
    PIP(R.string.quality_reason_pip),

    /** The video already fills the view at its own size (no upscaling happens), so the upscaler is off. */
    VIEWPORT(R.string.quality_reason_viewport),

    /** The video size is not known yet: shaders wait for it. */
    METADATA_PENDING(R.string.quality_reason_pending),
}

enum class QualityEventKind {
    /** One step lower (tier or preset), the chain still runs. */
    DOWNGRADED,

    /** The chain (or deband / smoothing) is off for this reason. */
    SUSPENDED,

    /** The reason went away: back to (closer to) what was requested. */
    RESTORED,

    /** FP16 Anime4K copies failed on this GPU: switched to full precision for this playback (same chain, no visible change). */
    PRECISION_FALLBACK,
}

/** One automatic change. [seq] grows by one per event of a session (show each once). [undoable] = [FRAME_DROPS][QualityReason.FRAME_DROPS]. */
data class QualityEvent(
    val seq: Int,
    val reason: QualityReason,
    val kind: QualityEventKind,
    val from: QualityLevel,
    val to: QualityLevel,
    val undoable: Boolean = false,
)

/** The automatic step-down in force that [PlayerSession.undoAutoDowngrade] can revert. */
data class AutoDowngrade(val from: QualityLevel, val to: QualityLevel, val reason: QualityReason)

/** One preset of the menu: [available] false with [unavailableReason] (ArtCNN on this build). */
data class PresetOption(val id: String, val available: Boolean, val unavailableReason: QualityReason? = null) {
    val tiered: Boolean get() = id in QualityPresets.TIERED
}

/** Family of the shaders running now (for the "what is processing" indicator). */
enum class ShaderFamily { NONE, ANIME4K, FIDELITY, FSR, CAS }

/**
 * What the quality UI renders. [requested] = the user's choices; [effective] = what runs after the guards, with [reasons] saying why
 * it differs (most important first). Telemetry fields come from mpv (polled every couple of seconds while playing).
 */
data class QualityUiState(
    /** False on Media3 (and External): every control is unavailable; the UI offers "Cambiar a MPV". */
    val available: Boolean = false,
    val playerEngine: PlayerEngine = PlayerEngine.MEDIA3,
    val requested: QualityRequest = QualityRequest(),
    val effective: QualityLevel = QualityLevel.OFF,
    val family: ShaderFamily = ShaderFamily.NONE,
    /** Shader file names in chain order (`Anime4K_Restore_CNN_M.glsl`...), empty when none runs. */
    val activeShaders: List<String> = emptyList(),
    val reasons: List<QualityReason> = emptyList(),
    val debandActive: Boolean = false,
    val sharpenActive: Boolean = false,
    /** Effective smoothing (requested and not forced off by frame generation, Link or battery). */
    val smoothingActive: Boolean = false,
    /** The frame-drop step-down in force, if any ([PlayerSession.undoAutoDowngrade]). */
    val autoDowngrade: AutoDowngrade? = null,
    /** mpv `hwdec-current`: `mediacodec` (HW+), `mediacodec-copy` (HW), `no` (software); null = unknown / Media3. */
    val decoderInUse: String? = null,
    /** Frames dropped in the current file (decoder + output). */
    val droppedFrames: Long = 0,
    val videoFps: Double? = null,
    val displayHz: Float? = null,
    /** `<vo> / <gpu-api>` (e.g. `gpu / opengl`); null = unknown. */
    val renderer: String? = null,
    val videoWidth: Int = 0,
    val videoHeight: Int = 0,
    val viewportWidth: Int = 0,
    val viewportHeight: Int = 0,
    val thermal: Boolean = false,
    val batteryLow: Boolean = false,
    val pip: Boolean = false,
    val remoteProcessing: Boolean = false,
    val frameGenActive: Boolean = false,
    /** Anime4K runs from the FP16 (mediump) copies. */
    val fp16: Boolean = false,
    val presets: List<PresetOption> = QualityPresets.ALL.map { PresetOption(it, available = false) },
    /** The last automatic change (show it once per [QualityEvent.seq]). */
    val lastEvent: QualityEvent? = null,
) {
    companion object {
        /** What a Media3 session publishes. */
        val MEDIA3 = QualityUiState()

        /** Shared, never-changing flow for engines without a quality engine. */
        val UNAVAILABLE: StateFlow<QualityUiState> = MutableStateFlow(MEDIA3).asStateFlow()
    }
}

/** es/en label of a decoder as mpv reports it in `hwdec-current` (`HW+`, `HW`, `SW`); null when unknown. */
fun decoderInUseLabel(hwdecCurrent: String?): String? = when (hwdecCurrent?.trim()) {
    null, "" -> null
    "no" -> "SW"
    "mediacodec" -> "HW+"
    "mediacodec-copy" -> "HW"
    else -> hwdecCurrent.trim()
}

/**
 * 0.4.0: whether the player can offer "Cambiar a MPV" (restart the same source in libmpv at the current position, for this playback
 * only): playing on Media3, libmpv bundled, and not a Link session (those need Media3's mTLS client).
 */
fun canSwitchToMpv(onMedia3: Boolean, mpvAvailable: Boolean, remote: Boolean): Boolean = onMedia3 && mpvAvailable && !remote
