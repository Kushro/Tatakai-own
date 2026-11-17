// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// 0.4.0 player quality (worker A, artifacts/player-quality-0.4.0-specs/a-quality-engine.md). Plain data shared by the settings
// (defaults, persisted by SettingsCodec), the libmpv quality engine (:core:player-mpv) and the in-player UI (:feature:player). The
// runtime state the UI renders lives in :core:player (`QualityUiState`).

/**
 * Upscaling preset ids, the same ids as desktop `tk-player/src/preset.rs` (`AppSettings.playerPreset`). Only the libmpv engine applies
 * them. [ARTCNN] needs compute shaders on gpu-next + Vulkan, which the bundled libmpv build does not have: it is listed so the UI can
 * show it disabled with its reason, and it never runs.
 */
object QualityPresets {
    const val OFF = "off"
    const val ANIME4K_A = "anime4k-a"
    const val ANIME4K_AA = "anime4k-aa"
    const val ANIME4K_B = "anime4k-b"
    const val ANIME4K_C = "anime4k-c"
    const val ANIME4K_BB = "anime4k-bb"
    const val ANIME4K_CA = "anime4k-ca"
    const val FSR = "fsr"
    const val FIDELITY = "fidelity"
    const val CAS = "cas"
    const val ARTCNN = "artcnn"

    /** Every id, in the order menus show them (desktop order, ArtCNN last). */
    val ALL: List<String> = listOf(OFF, ANIME4K_A, ANIME4K_AA, ANIME4K_B, ANIME4K_C, ANIME4K_BB, ANIME4K_CA, FSR, FIDELITY, CAS, ARTCNN)

    /** Presets whose chain depends on [QualityTier] (network sizes). FSR / CAS have a single chain. */
    val TIERED: Set<String> = setOf(ANIME4K_A, ANIME4K_AA, ANIME4K_B, ANIME4K_C, ANIME4K_BB, ANIME4K_CA, FIDELITY)

    fun isAnime4K(id: String): Boolean = id.startsWith("anime4k-") && id in ALL

    /** A stored / requested id, or [OFF] when unknown. */
    fun sanitize(id: String?): String = id?.takeIf { it in ALL } ?: OFF
}

/**
 * Quality tier of a tiered preset (network size of Anime4K, depth of the Fidelity chain). Declared cheapest first: the automatic
 * step-down walks [ordinal] down (HQ -> BALANCED -> FAST -> shaders off).
 *
 * Anime4K (big network, small network): FAST = S/S, BALANCED = M/S (the chains Tatakai shipped until 0.3.x, desktop "Fast"),
 * HQ = L/M (desktop HQ uses VL/M; VL is too heavy for phones, Enigmata's HQ tier uses L).
 */
@Serializable
enum class QualityTier {
    @SerialName("fast") FAST,
    @SerialName("balanced") BALANCED,
    @SerialName("hq") HQ,
    ;

    /** One step cheaper; null below [FAST] (= shaders off). */
    fun lower(): QualityTier? = entries.getOrNull(ordinal - 1)
}

/**
 * Video decoder (mpv `hwdec`). [AUTO] = hardware with copy-back first, direct MediaCodec next, software last (the order Tatakai always
 * used); [HW] = MediaCodec with copy-back (shaders and screenshots work); [HW_PLUS] = direct MediaCodec (AImageReader zero-copy, least
 * power); [SW] = FFmpeg software decoding. mpv falls back to software by itself when a hardware decoder refuses the stream.
 */
@Serializable
enum class DecoderMode(val hwdec: String) {
    @SerialName("auto") AUTO("mediacodec-copy,mediacodec,no"),
    @SerialName("hw") HW("mediacodec-copy"),
    @SerialName("hw_plus") HW_PLUS("mediacodec"),
    @SerialName("sw") SW("no"),
}

/** mpv `deband*` (vo=gpu). Defaults = Enigmata's anime profile (2 / 40 / 20 / 6). */
@Serializable
data class DebandConfig(
    val enabled: Boolean = true,
    val iterations: Int = 2,
    val threshold: Int = 40,
    val range: Int = 20,
    val grain: Int = 6,
) {
    /** mpv's accepted ranges (iterations 0..16, threshold 0..4096, range 1..64, grain 0..4096; the UI offers 0..200 for the last). */
    fun sanitized(): DebandConfig = copy(
        iterations = iterations.coerceIn(0, 16),
        threshold = threshold.coerceIn(0, 200),
        range = range.coerceIn(1, 64),
        grain = grain.coerceIn(0, 200),
    )
}

/** mpv `brightness` / `contrast` / `saturation` / `gamma` / `hue`, each -100..100, 0 = unchanged. */
@Serializable
data class ImageAdjust(
    val brightness: Int = 0,
    val contrast: Int = 0,
    val saturation: Int = 0,
    val gamma: Int = 0,
    val hue: Int = 0,
) {
    fun sanitized(): ImageAdjust = ImageAdjust(
        brightness.coerceIn(MIN, MAX),
        contrast.coerceIn(MIN, MAX),
        saturation.coerceIn(MIN, MAX),
        gamma.coerceIn(MIN, MAX),
        hue.coerceIn(MIN, MAX),
    )

    val isNeutral: Boolean get() = this == ImageAdjust()

    companion object {
        const val MIN = -100
        const val MAX = 100
    }
}

/** How the video fills the view. [FILL] crops to cover it (mpv `panscan=1`); the ratios force the display aspect. */
@Serializable
enum class AspectMode {
    @SerialName("fit") FIT,
    @SerialName("fill") FILL,
    @SerialName("stretch") STRETCH,
    @SerialName("16_9") RATIO_16_9,
    @SerialName("4_3") RATIO_4_3,
}

/** Sharpening (FidelityFX CAS on the luma plane; [OFF] = no shader). The `cas` preset uses [MEDIUM] when this is [OFF]. */
@Serializable
enum class SharpenLevel(val casSharpening: Double) {
    @SerialName("off") OFF(0.0),
    @SerialName("low") LOW(0.0),
    @SerialName("medium") MEDIUM(0.5),
    @SerialName("high") HIGH(1.0),
}

/**
 * Settings › Reproductor defaults of the quality engine (the preset itself stays in `AppSettings.playerPreset`). Each new playback
 * starts from these; changes made in the player last for that playback only unless the UI saves them here.
 */
@Serializable
data class PlayerQualityPrefs(
    val tier: QualityTier = QualityTier.BALANCED,
    val deband: DebandConfig = DebandConfig(),
    /** Thermal protection, battery saver (<= 20 % unplugged) and the frame-drop step-down. The 4K / 8K / view guards always apply. */
    val autoAdjust: Boolean = true,
    val decoder: DecoderMode = DecoderMode.AUTO,
    /** mpv `interpolation` + `display-resample` + `tscale=oversample`: frame pacing, not real frame generation. */
    val smoothing: Boolean = false,
    val sharpen: SharpenLevel = SharpenLevel.OFF,
) {
    fun sanitized(): PlayerQualityPrefs = copy(deband = deband.sanitized())
}
