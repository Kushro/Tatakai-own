// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.content.Context
import java.io.File

// parity wave A (docs/redesign/15 §0.11, checklist player.android.upscaling-shaders). Owner: worker p.
// The official Anime4K v4 chains (the "Fast" M/S variants for mobile GPUs) and AMD FSR, the same ids and order as desktop
// `apps/desktop-native/crates/tk-player/src/preset.rs`. Every file is bundled in `assets/shaders/` (licences in THIRD_PARTY_NOTICES.md).

object ShaderPresets {
    private const val CLAMP = "Anime4K_Clamp_Highlights.glsl"
    private const val RESTORE_M = "Anime4K_Restore_CNN_M.glsl"
    private const val RESTORE_S = "Anime4K_Restore_CNN_S.glsl"
    private const val RESTORE_SOFT_M = "Anime4K_Restore_CNN_Soft_M.glsl"
    private const val UPSCALE_M = "Anime4K_Upscale_CNN_x2_M.glsl"
    private const val UPSCALE_S = "Anime4K_Upscale_CNN_x2_S.glsl"
    private const val UPSCALE_DENOISE_M = "Anime4K_Upscale_Denoise_CNN_x2_M.glsl"
    private const val DOWNSCALE_X2 = "Anime4K_AutoDownscalePre_x2.glsl"
    private const val FSR = "FSR.glsl"

    /** Asset folder of the bundled shaders. */
    const val ASSET_DIR = "shaders"

    /** Preset id (`AppSettings.playerPreset`) -> shader files in chain order. */
    val CHAINS: Map<String, List<String>> = mapOf(
        "off" to emptyList(),
        "anime4k-a" to listOf(CLAMP, RESTORE_M, UPSCALE_M, DOWNSCALE_X2, UPSCALE_S),
        "anime4k-aa" to listOf(CLAMP, RESTORE_M, UPSCALE_M, RESTORE_S, DOWNSCALE_X2, UPSCALE_S),
        "anime4k-b" to listOf(CLAMP, RESTORE_SOFT_M, UPSCALE_M, DOWNSCALE_X2, UPSCALE_S),
        "anime4k-c" to listOf(CLAMP, UPSCALE_DENOISE_M, DOWNSCALE_X2, UPSCALE_S),
        "fsr" to listOf(FSR),
    )

    /** Every bundled shader (each must exist in [ASSET_DIR]). */
    val ALL_FILES: Set<String> = CHAINS.values.flatten().toSet()

    fun chain(preset: String): List<String> = CHAINS[preset].orEmpty()

    /**
     * Target names inside `filesDir/shaders/<preset>/`: an index prefix keeps the chain order when listed (`00_Anime4K_Clamp...glsl`), so a
     * shader used twice in one chain never collides.
     */
    fun targetNames(preset: String): List<String> = chain(preset).mapIndexed { i, f -> "%02d_%s".format(i, f) }

    /**
     * Copies the chain of [preset] from the assets to `filesDir/shaders/<preset>/` when missing (or of another size) and returns the files
     * in chain order. Empty for "off" / unknown ids. Runs on the caller's thread (small files, copied once per install).
     */
    fun install(context: Context, preset: String): List<File> {
        val files = chain(preset)
        if (files.isEmpty()) return emptyList()
        val dir = File(context.filesDir, "shaders/$preset").apply { mkdirs() }
        val assets = context.assets
        return files.zip(targetNames(preset)).map { (asset, name) ->
            val target = File(dir, name)
            val expected = runCatching { assets.openFd("$ASSET_DIR/$asset").use { it.length } }.getOrDefault(-1L)
            if (!target.isFile || (expected > 0 && target.length() != expected)) {
                val tmp = File(dir, "$name.tmp")
                assets.open("$ASSET_DIR/$asset").use { input -> tmp.outputStream().use { input.copyTo(it) } }
                if (!tmp.renameTo(target)) {
                    tmp.copyTo(target, overwrite = true)
                    tmp.delete()
                }
            }
            target
        }
    }
}
