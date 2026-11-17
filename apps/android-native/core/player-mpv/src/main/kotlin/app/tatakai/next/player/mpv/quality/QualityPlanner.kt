// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.model.SharpenLevel
import app.tatakai.next.player.QualityLevel
import app.tatakai.next.player.QualityReason
import app.tatakai.next.player.QualityRequest
import app.tatakai.next.player.ShaderFamily

// 0.4.0 (worker A): the pure decision table of the quality engine, adapted from Enigmata `domain/quality/{ProfileEngine,QualityPlan}.kt`
// (AGPL-3.0-or-later, same author). Tatakai only plays anime, so there is no content detection and no live-action branch; the guards
// keep Enigmata's rules (8K off, 4K no upscaling, thermal one tier down but never off, Eco off on low battery, PiP capped, no upscaling
// when the video already fills the view) plus the two hooks of the other 0.4.0 workers: [QualitySignals.remoteProcessing] (Link: the
// PC processes the video) and [QualitySignals.frameGenActive] (phone frame generation owns the frame pacing).

/** Coarse resolution bucket of the source ([fromDimensions] uses the short side, so portrait clips classify like landscape ones). */
enum class ResolutionTier {
    UNKNOWN, SD, HD, FHD, QHD, UHD4K, UHD8K;

    companion object {
        fun fromDimensions(w: Int, h: Int): ResolutionTier {
            if (w <= 0 || h <= 0) return UNKNOWN
            val effHeight = minOf(w, h)
            val effWidth = maxOf(w, h)
            return when {
                effWidth > 3840 || effHeight > 2160 -> UHD8K
                effHeight > 1450 -> UHD4K
                effHeight > 1080 -> QHD
                effHeight > 720 -> FHD
                effHeight >= 577 -> HD
                else -> SD
            }
        }
    }
}

/**
 * The frame-drop step-down in force for this playback (never persisted). [tierCap] caps the tier of tiered presets; [chainOff] turns
 * the whole chain off; [extrasOff] also turns smoothing, sharpen and deband off (the last step, when no chain runs).
 */
data class DropCeiling(val tierCap: QualityTier? = null, val chainOff: Boolean = false, val extrasOff: Boolean = false)

/** Everything besides the user's request that decides what runs. */
data class QualitySignals(
    val videoWidth: Int = 0,
    val videoHeight: Int = 0,
    val viewportWidth: Int = 0,
    val viewportHeight: Int = 0,
    val pip: Boolean = false,
    /** Thermal verdict of [ThermalPolicy] (applied only with `autoAdjust`). */
    val thermal: Boolean = false,
    /** Battery <= 20 % and not charging (applied only with `autoAdjust`). */
    val batteryLow: Boolean = false,
    val dropCeiling: DropCeiling? = null,
    val remoteProcessing: Boolean = false,
    val frameGenActive: Boolean = false,
    /** A shader of the current chain failed to compile: the chain stays off for this file. */
    val shaderError: Boolean = false,
    /** The renderer can run `//!COMPUTE` shaders (gpu-next + Vulkan). False with the bundled libmpv. */
    val supportsCompute: Boolean = false,
)

/** What to apply: the effective level, the shader passes, the extras and why it differs from the request. */
data class QualityPlan(
    val level: QualityLevel,
    val family: ShaderFamily,
    val shaders: List<ShaderSpec>,
    val deband: Boolean,
    val sharpen: Boolean,
    val interpolation: Boolean,
    /** Most important first ([QualityReason] declaration order). */
    val reasons: List<QualityReason>,
)

object QualityPlanner {
    fun plan(request: QualityRequest, s: QualitySignals): QualityPlan {
        val reasons = linkedSetOf<QualityReason>()
        var preset = QualityPresets.sanitize(request.preset)
        var tier: QualityTier = request.tier
        var deband = request.deband.enabled
        var sharpen = request.sharpen != SharpenLevel.OFF
        var smoothing = request.smoothing
        val auto = request.autoAdjust

        ShaderCatalog.unsupported(preset, s.supportsCompute)?.let { reasons += it; preset = QualityPresets.OFF }
        if (s.shaderError && (preset != QualityPresets.OFF || sharpen)) {
            reasons += QualityReason.SHADER_ERROR
            preset = QualityPresets.OFF
            sharpen = false
        }
        val wantsChain = preset != QualityPresets.OFF || sharpen

        if (s.remoteProcessing) {
            if (wantsChain || deband || smoothing) reasons += QualityReason.REMOTE_PROCESSING
            preset = QualityPresets.OFF
            sharpen = false
            deband = false
            smoothing = false
        }
        when (ResolutionTier.fromDimensions(s.videoWidth, s.videoHeight)) {
            ResolutionTier.UHD8K -> {
                if (wantsChain || deband) reasons += QualityReason.RES_8K
                preset = QualityPresets.OFF
                sharpen = false
                deband = false
            }
            ResolutionTier.UHD4K -> {
                // A 4K source on a <= 2.8K phone panel is downscaled: upscalers do nothing useful, and deband / sharpen run on 8 M pixels.
                if (wantsChain || deband) reasons += QualityReason.RES_4K
                preset = QualityPresets.OFF
                sharpen = false
                deband = false
            }
            ResolutionTier.UNKNOWN -> if (preset != QualityPresets.OFF) {
                // Never load a heavy chain before the size is known (it may be 4K / 8K, or already fill the view).
                reasons += QualityReason.METADATA_PENDING
                preset = QualityPresets.OFF
            }
            else -> Unit
        }
        if (auto && s.batteryLow) {
            if (preset != QualityPresets.OFF || sharpen || deband || smoothing) reasons += QualityReason.BATTERY_LOW
            preset = QualityPresets.OFF
            sharpen = false
            deband = false
            smoothing = false
        }
        if (s.frameGenActive && smoothing) {
            reasons += QualityReason.FRAME_GEN
            smoothing = false
        }
        val tiered = preset in QualityPresets.TIERED
        if (auto && s.thermal && tiered && tier > QualityTier.FAST) {
            // Thermal alone never turns the chain off (Enigmata's rule): one tier down, FAST at most.
            tier = tier.lower() ?: QualityTier.FAST
            reasons += QualityReason.THERMAL
        }
        if (s.pip && tiered && tier > QualityTier.FAST) {
            tier = QualityTier.FAST
            reasons += QualityReason.PIP
        }
        val ceiling = s.dropCeiling.takeIf { auto }
        if (ceiling != null) {
            if (ceiling.chainOff && preset != QualityPresets.OFF) {
                preset = QualityPresets.OFF
                reasons += QualityReason.FRAME_DROPS
            }
            val cap = ceiling.tierCap
            if (cap != null && tiered && preset != QualityPresets.OFF && tier > cap) {
                tier = cap
                reasons += QualityReason.FRAME_DROPS
            }
            if (ceiling.extrasOff && (sharpen || deband || smoothing)) {
                sharpen = false
                deband = false
                smoothing = false
                reasons += QualityReason.FRAME_DROPS
            }
        }

        var files = ShaderCatalog.chain(preset, tier)
        if (files.isNotEmpty() && !upscalesInView(s)) {
            val kept = files.filterNot { ShaderCatalog.isUpscaler(it) }
            if (kept.size != files.size) reasons += QualityReason.VIEWPORT
            // Anime4K / FSR are pure upscalers: nothing of them is left. Fidelity keeps its chroma / downscaling passes.
            files = kept
            if (files.isEmpty()) preset = QualityPresets.OFF
        }
        val specs = files.map { f ->
            if (f == ShaderCatalog.CAS) ShaderSpec(f, ShaderTransform.CAS, casLevel(request.sharpen)) else ShaderSpec(f)
        }.toMutableList()
        val sharpenPass = sharpen && preset != QualityPresets.CAS
        if (sharpenPass) specs += ShaderSpec(ShaderCatalog.CAS, ShaderTransform.CAS, request.sharpen.casSharpening)
        val level = if (files.isEmpty()) QualityLevel.OFF else QualityLevel.of(preset, tier)
        return QualityPlan(
            level = level,
            family = if (files.isEmpty()) ShaderFamily.NONE else ShaderCatalog.family(preset),
            shaders = specs,
            deband = deband,
            sharpen = sharpenPass || (preset == QualityPresets.CAS && files.isNotEmpty()),
            interpolation = smoothing,
            reasons = reasons.sortedBy { it.ordinal },
        )
    }

    /** The `cas` preset sharpens at the chosen level, or [SharpenLevel.MEDIUM] when the sharpen knob is off. */
    private fun casLevel(level: SharpenLevel): Double = if (level == SharpenLevel.OFF) SharpenLevel.MEDIUM.casSharpening else level.casSharpening

    /**
     * Whether the (fit) video is scaled up in the view; unknown sizes count as yes (no guard). Same rule as Enigmata's
     * `upscaleBenefitsViewport`, without rotation / pixel aspect (mpv reports the display size of the decoded frame already).
     */
    fun upscalesInView(s: QualitySignals): Boolean {
        if (s.videoWidth <= 0 || s.videoHeight <= 0 || s.viewportWidth <= 0 || s.viewportHeight <= 0) return true
        val scale = minOf(s.viewportWidth.toDouble() / s.videoWidth, s.viewportHeight.toDouble() / s.videoHeight)
        return scale > 1.0
    }

    /**
     * The next frame-drop step from the plan that dropped frames: tiered chain above FAST -> one tier lower; any chain -> off; no chain
     * but smoothing / sharpen / deband -> those off. Null when there is nothing left to lower.
     */
    fun nextDropCeiling(current: DropCeiling?, plan: QualityPlan): DropCeiling? {
        val base = current ?: DropCeiling()
        val tier = plan.level.tier
        return when {
            !plan.level.off && tier != null && tier > QualityTier.FAST -> base.copy(tierCap = tier.lower())
            !plan.level.off -> base.copy(chainOff = true)
            (plan.interpolation || plan.deband || plan.sharpen) && !base.extrasOff -> base.copy(extrasOff = true)
            else -> null
        }
    }
}
