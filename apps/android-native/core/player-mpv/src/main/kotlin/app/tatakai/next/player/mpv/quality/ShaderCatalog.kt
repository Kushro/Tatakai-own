// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.player.QualityReason
import app.tatakai.next.player.ShaderFamily

// 0.4.0 (worker A): every preset's shader chain per tier. Chain shapes follow desktop `tk-player/src/preset.rs` (same ids, same order
// of passes; desktop also adds AutoDownscalePre_x4, which only matters past 4x and is not bundled here). The Fidelity tiers come from
// Enigmata `domain/quality/ShaderChainManager.kt` (AGPL-3.0-or-later, same author). Pure: no Android, unit tested.

/** How a bundled file is turned into the file mpv loads. */
enum class ShaderTransform {
    /** Copied as shipped (FP16 rewrite of `Anime4K_*` files decided by the resolver). */
    NONE,

    /** FidelityFX CAS with its `WHEN` relaxed to run on scaled video and `SHARPENING` set from [ShaderSpec.casSharpening]. */
    CAS,
}

/** One pass of the chain: a bundled asset (`assets/shaders/<asset>`) and how it is installed. */
data class ShaderSpec(val asset: String, val transform: ShaderTransform = ShaderTransform.NONE, val casSharpening: Double = 0.0)

object ShaderCatalog {
    const val ASSET_DIR = "shaders"

    const val CLAMP = "Anime4K_Clamp_Highlights.glsl"
    const val DOWN_X2 = "Anime4K_AutoDownscalePre_x2.glsl"
    const val FSR = "FSR.glsl"
    const val CAS = "CAS.glsl"
    const val FSRCNNX_LINEART = "FSRCNNX_x2_8-0-4-1_LineArt.glsl"
    const val KRIG = "KrigBilateral.glsl"
    const val SSIM_SUPER = "SSimSuperRes.glsl"
    const val SSIM_DOWN = "SSimDownscaler.glsl"

    private fun restore(v: String) = "Anime4K_Restore_CNN_$v.glsl"
    private fun soft(v: String) = "Anime4K_Restore_CNN_Soft_$v.glsl"
    private fun up(v: String) = "Anime4K_Upscale_CNN_x2_$v.glsl"
    private fun denoise(v: String) = "Anime4K_Upscale_Denoise_CNN_x2_$v.glsl"

    /**
     * (big, small) Anime4K network sizes of [tier]. BALANCED = M/S = the chains Tatakai shipped until 0.3.x (desktop "Fast"); HQ = L/M
     * (desktop HQ uses VL/M, too heavy for a phone GPU); FAST = S/S (the old "lite" fallback).
     */
    fun networks(tier: QualityTier): Pair<String, String> = when (tier) {
        QualityTier.FAST -> "S" to "S"
        QualityTier.BALANCED -> "M" to "S"
        QualityTier.HQ -> "L" to "M"
    }

    /** Shader files of [preset] at [tier] in application order (no sharpen pass). Empty for off / unknown / unsupported presets. */
    fun chain(preset: String, tier: QualityTier): List<String> {
        val (big, small) = networks(tier)
        return when (preset) {
            QualityPresets.ANIME4K_A -> listOf(CLAMP, restore(big), up(big), DOWN_X2, up(small))
            QualityPresets.ANIME4K_AA -> listOf(CLAMP, restore(big), up(big), restore(small), DOWN_X2, up(small))
            QualityPresets.ANIME4K_B -> listOf(CLAMP, soft(big), up(big), DOWN_X2, up(small))
            QualityPresets.ANIME4K_C -> listOf(CLAMP, denoise(big), DOWN_X2, up(small))
            QualityPresets.ANIME4K_BB -> listOf(CLAMP, soft(big), up(big), DOWN_X2, soft(small), up(small))
            QualityPresets.ANIME4K_CA -> listOf(CLAMP, denoise(big), DOWN_X2, restore(small), up(small))
            QualityPresets.FSR -> listOf(FSR)
            QualityPresets.FIDELITY -> fidelity(tier)
            QualityPresets.CAS -> listOf(CAS)
            else -> emptyList()
        }
    }

    /**
     * Enigmata's tiered Fidelity chain for anime (FSRCNNX LineArt): HQ adds the chroma (KrigBilateral) and downscaling (SSimDownscaler)
     * passes of desktop's Fidelity, BALANCED is FSRCNNX + SSimSuperRes, FAST only SSimSuperRes.
     */
    private fun fidelity(tier: QualityTier): List<String> = when (tier) {
        QualityTier.HQ -> listOf(FSRCNNX_LINEART, KRIG, SSIM_SUPER, SSIM_DOWN)
        QualityTier.BALANCED -> listOf(FSRCNNX_LINEART, SSIM_SUPER)
        QualityTier.FAST -> listOf(SSIM_SUPER)
    }

    /** Passes that only make sense when the video is scaled UP (dropped by the view guard). */
    fun isUpscaler(file: String): Boolean =
        file.startsWith("Anime4K_") || file == FSR || file == FSRCNNX_LINEART || file == SSIM_SUPER

    /** Why [preset] can never run with this libmpv build, or null. ArtCNN uses `//!COMPUTE` passes (gpu-next + Vulkan only). */
    fun unsupported(preset: String, supportsCompute: Boolean = false): QualityReason? =
        if (preset == QualityPresets.ARTCNN && !supportsCompute) QualityReason.RENDERER_UNSUPPORTED else null

    fun family(preset: String): ShaderFamily = when {
        QualityPresets.isAnime4K(preset) -> ShaderFamily.ANIME4K
        preset == QualityPresets.FIDELITY -> ShaderFamily.FIDELITY
        preset == QualityPresets.FSR -> ShaderFamily.FSR
        preset == QualityPresets.CAS -> ShaderFamily.CAS
        else -> ShaderFamily.NONE
    }

    /** Every bundled file a preset or the sharpen pass can ask for (each must exist in `assets/shaders/`). */
    val ALL_FILES: Set<String> = buildSet {
        for (p in QualityPresets.ALL) for (t in QualityTier.entries) addAll(chain(p, t))
        add(CAS)
    }
}
