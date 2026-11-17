// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.Serializable

// 0.4.0 player quality (worker B): settings of the phone frame generation (:core:framegen, LSFG ported from Enigmata R13).
// Kept in their own data class (AppSettings.frameGen) so the quality-engine settings of worker A stay separate. Persisted by
// SettingsCodec as one JSON key (`frame_gen_json`); values out of range are fixed by [FrameGenSettings.sanitized].
// The mapping to Enigmata's `FgPreferences` keys is 1:1 (same defaults).

/**
 * Frame generation performance profile (Enigmata `FgProfile`, fact 25). SAVER = performance mode, flow 0.5, input at 75 % of
 * the screen; BALANCED = performance mode, flow 0.5, full input (default); QUALITY = quality mode (LSFG 3.1), flow 1.0, full
 * input; CUSTOM = [FrameGenSettings.customPerformance] / [FrameGenSettings.customFlowScale] / [FrameGenSettings.customInputScale].
 */
@Serializable
enum class FrameGenProfile { SAVER, BALANCED, QUALITY, CUSTOM }

/**
 * What FG gives up when the upscaler (Anime4K / FSR) and the GPU budget compete (Enigmata `FgPriority`, FIX-5): FLUIDITY keeps
 * the multiplier and lowers the intermediate input size; SHARPNESS caps the multiplier at 2x.
 */
@Serializable
enum class FrameGenPriority { FLUIDITY, SHARPNESS }

/**
 * Global (never per file) frame generation settings.
 *
 * @property enabled the user switch; the next video starts with FG when it is eligible.
 * @property multiplier [MULTIPLIER_AUTO] (0) or one of [MULTIPLIER_RANGE] (2..5).
 * @property matchRefresh ask the display for fps x N (`Surface.setFrameRate`, may switch the mode) while FG runs.
 * @property audioOffsetMs manual audio offset added on top of the measured pipeline latency, [AUDIO_OFFSET_RANGE_MS].
 * @property allowUntestedVulkan also allow Vulkan 1.1 / 1.2 GPUs (always marked experimental).
 * @property showStats show the FG stats overlay in the player (and a logcat line about once per second).
 * @property videoFitFloor floor of the video-fit input size (0, 0.5, 0.75, 1.0).
 * @property screenResolution FG input = screen x profile scale even when the GPU budget is exceeded.
 */
@Serializable
data class FrameGenSettings(
    val enabled: Boolean = false,
    val multiplier: Int = MULTIPLIER_AUTO,
    val profile: FrameGenProfile = FrameGenProfile.BALANCED,
    val customPerformance: Boolean = true,
    val customFlowScale: Float = 0.5f,
    val customInputScale: Float = 1f,
    val matchRefresh: Boolean = true,
    val audioOffsetMs: Int = 0,
    val allowUntestedVulkan: Boolean = false,
    val showStats: Boolean = false,
    val videoFitFloor: Float = 1f,
    val priority: FrameGenPriority = FrameGenPriority.FLUIDITY,
    val screenResolution: Boolean = false,
) {
    /** Values a hand-edited or older store may hold out of range, brought back to the accepted ones. */
    fun sanitized(): FrameGenSettings = copy(
        multiplier = multiplier.takeIf { it == MULTIPLIER_AUTO || it in MULTIPLIER_RANGE } ?: MULTIPLIER_AUTO,
        customFlowScale = if (customFlowScale.isFinite()) customFlowScale.coerceIn(0.25f, 1f) else 0.5f,
        customInputScale = if (customInputScale == 0.75f) 0.75f else 1f,
        audioOffsetMs = audioOffsetMs.coerceIn(AUDIO_OFFSET_RANGE_MS.first, AUDIO_OFFSET_RANGE_MS.last),
        videoFitFloor = VIDEO_FIT_FLOORS.minBy { kotlin.math.abs(it - (if (videoFitFloor.isFinite()) videoFitFloor else 1f)) },
    )

    companion object {
        const val MULTIPLIER_AUTO = 0
        val MULTIPLIER_RANGE = 2..5
        val AUDIO_OFFSET_RANGE_MS = -500..500
        val VIDEO_FIT_FLOORS = listOf(0f, 0.5f, 0.75f, 1f)
    }
}
