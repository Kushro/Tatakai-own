// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import java.util.Locale

// 0.4.0 (worker A): text transforms of bundled shaders and shader-error detection. Pure, unit tested.

object ShaderSources {
    /**
     * The `WHEN` that agyild's CAS.glsl itself documents for use on scaled content ("allows it to be used as a pre-upscale
     * sharpener"): run unless the output is smaller than the source. The shipped `WHEN` only runs on unscaled video, which on a phone
     * (almost always scaled to the panel) would make CAS a no-op.
     */
    const val CAS_SCALED_WHEN = "//!WHEN OUTPUT.w OUTPUT.h * LUMA.w LUMA.h * / 1.0 < !"

    private val WHEN_LINE = Regex("""(?m)^//!WHEN .*$""")
    private val SHARPENING = Regex("""(?m)^#define SHARPENING [0-9.]+""")

    /** CAS.glsl with every `WHEN` relaxed to [CAS_SCALED_WHEN] and `SHARPENING` set to [sharpening] (0.0..1.0). */
    fun casVariant(src: String, sharpening: Double): String {
        val s = String.format(Locale.ROOT, "%.2f", sharpening.coerceIn(0.0, 1.0))
        return src.replace("\r\n", "\n")
            .replace(WHEN_LINE, CAS_SCALED_WHEN)
            .replace(SHARPENING, "#define SHARPENING $s")
    }

    /** File-name tag of a CAS variant (`CAS_s050.glsl`). */
    fun casFileName(sharpening: Double): String = String.format(Locale.ROOT, "CAS_s%03d.glsl", (sharpening.coerceIn(0.0, 1.0) * 100).toInt())
}

/** mpv's raw `mpv_log_level` values (also `MPVLib.MpvLogLevel`): FATAL and ERROR. */
private const val MPV_LOG_LEVEL_FATAL = 10
private const val MPV_LOG_LEVEL_ERROR = 20

/**
 * Adapted from Enigmata `domain/quality/ShaderDiagnostics.kt` (AGPL-3.0-or-later, same author): only explicit fatal / error
 * compile / link reports count (the OpenGL link failure `vo/gpu/opengl` "shader link log (status=0)", libplacebo's render-pass failure),
 * never a configured shader path or a shader source dump.
 */
fun isShaderCompilationError(level: Int, text: String): Boolean {
    if (level != MPV_LOG_LEVEL_FATAL && level != MPV_LOG_LEVEL_ERROR) return false
    val t = text.lowercase()
    return (t.contains("shader") && t.contains("compil")) ||
        t.contains("shader link log") ||
        (t.contains("glsl") && (t.contains("error") || t.contains("fail"))) ||
        t.contains("shaderc output") ||
        t.contains("failed creating render pass") ||
        (t.contains("link") && t.contains("program") && (t.contains("fail") || t.contains("error")))
}
