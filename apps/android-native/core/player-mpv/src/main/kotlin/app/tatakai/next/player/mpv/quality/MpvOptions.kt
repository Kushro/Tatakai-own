// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import app.tatakai.next.model.AspectMode
import app.tatakai.next.model.DebandConfig
import app.tatakai.next.model.ImageAdjust
import java.util.Locale
import kotlin.math.ln

// 0.4.0 (worker A): the mpv property values of each live knob. Pure tables (written to the running mpv by `MpvQualityController` as
// runtime properties, so nothing needs a reload). Interpolation and deband values follow Enigmata
// `ui/player/mpvoptions/MpvOptionsApplier.kt` (`interpolationOptions`, `applyDebandParams`; AGPL-3.0-or-later, same author).
object MpvOptions {
    /** `deband` + its four parameters (the parameters are written even when off, so turning it on later is a single flag). */
    fun deband(active: Boolean, cfg: DebandConfig): Map<String, String> {
        val c = cfg.sanitized()
        return linkedMapOf(
            "deband" to yesNo(active),
            "deband-iterations" to c.iterations.toString(),
            "deband-threshold" to c.threshold.toString(),
            "deband-range" to c.range.toString(),
            "deband-grain" to c.grain.toString(),
        )
    }

    fun image(image: ImageAdjust): Map<String, String> {
        val i = image.sanitized()
        return linkedMapOf(
            "brightness" to i.brightness.toString(),
            "contrast" to i.contrast.toString(),
            "saturation" to i.saturation.toString(),
            "gamma" to i.gamma.toString(),
            "hue" to i.hue.toString(),
        )
    }

    /**
     * Fit = mpv's default; fill = `panscan=1` (crop to cover the view); stretch = `keepaspect=no`; the ratios force the display aspect
     * (`video-aspect-override`). Zoom is `video-zoom`, a log2 factor (100 % = 0, 200 % = 1).
     */
    fun aspect(mode: AspectMode, zoomPercent: Int): Map<String, String> = linkedMapOf(
        "keepaspect" to yesNo(mode != AspectMode.STRETCH),
        "panscan" to if (mode == AspectMode.FILL) "1.0" else "0.0",
        "video-aspect-override" to when (mode) {
            AspectMode.RATIO_16_9 -> "16:9"
            AspectMode.RATIO_4_3 -> "4:3"
            else -> "no"
        },
        "video-zoom" to String.format(Locale.ROOT, "%.4f", ln(zoomPercent.coerceIn(25, 400) / 100.0) / ln(2.0)),
    )

    /**
     * Smoothing ON: `video-sync=display-resample` (what `interpolation=yes` needs to have any effect) + `tscale=oversample`, and the
     * panel's refresh rate as `display-fps-override` when known (the Android GL context does not report it to mpv). OFF restores mpv's
     * defaults (`video-sync=audio`, no override), so A/V sync is never left display-driven.
     */
    fun interpolation(enabled: Boolean, displayHz: Float?): Map<String, String> = if (enabled) {
        linkedMapOf(
            "video-sync" to "display-resample",
            "interpolation" to "yes",
            "tscale" to "oversample",
            "display-fps-override" to (displayHz?.takeIf { it > 0f }?.let { String.format(Locale.ROOT, "%.3f", it) } ?: "0"),
        )
    } else {
        linkedMapOf("interpolation" to "no", "video-sync" to "audio", "display-fps-override" to "0")
    }

    /** mpv's `:`-separated path list (POSIX), as `glsl-shaders` wants it. */
    fun shaderList(paths: List<String>): String = paths.joinToString(":")

    private fun yesNo(b: Boolean) = if (b) "yes" else "no"
}
