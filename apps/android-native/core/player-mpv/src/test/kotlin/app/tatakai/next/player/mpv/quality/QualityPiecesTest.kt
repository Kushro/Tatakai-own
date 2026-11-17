// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

import app.tatakai.next.model.AspectMode
import app.tatakai.next.model.DebandConfig
import app.tatakai.next.model.ImageAdjust
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

// 0.4.0 (worker A): frame-drop windows, mpv option tables, CAS variants, installed file names, shader-error detection.
class QualityPiecesTest {
    @Test
    fun dropMonitorNeedsAFullWindowAndARatio() {
        val m = FrameDropMonitor()
        assertFalse(m.sample(0, 24.0, 0))
        assertFalse(m.sample(100, 24.0, 4_000), "window not closed yet")
        // 8 s at 24 fps = 192 frames; 100 dropped = 52 %.
        assertTrue(m.sample(100, 24.0, 8_000))
        // Next window: 10 drops in 8 s is fine.
        assertFalse(m.sample(110, 24.0, 16_000))
        // A short hiccup below the absolute minimum never counts, whatever the ratio.
        val slow = FrameDropMonitor()
        slow.sample(0, 1.0, 0)
        assertFalse(slow.sample(5, 1.0, 8_000))
    }

    @Test
    fun dropMonitorResetsOnCounterRestart() {
        val m = FrameDropMonitor()
        m.sample(500, 24.0, 0)
        // New file: counters start again from 0.
        assertFalse(m.sample(3, 24.0, 9_000))
        assertFalse(m.sample(40, 24.0, 12_000))
        assertTrue(m.sample(80, 24.0, 17_000))
    }

    @Test
    fun debandAndImageOptions() {
        val d = MpvOptions.deband(true, DebandConfig())
        assertEquals("yes", d["deband"])
        assertEquals(listOf("2", "40", "20", "6"), listOf("deband-iterations", "deband-threshold", "deband-range", "deband-grain").map { d[it] })
        assertEquals("no", MpvOptions.deband(false, DebandConfig())["deband"])
        val img = MpvOptions.image(ImageAdjust(brightness = 150, contrast = -5, gamma = 3))
        assertEquals("100", img["brightness"])
        assertEquals("-5", img["contrast"])
        assertEquals("0", img["hue"])
    }

    @Test
    fun aspectOptions() {
        assertEquals(mapOf("keepaspect" to "yes", "panscan" to "0.0", "video-aspect-override" to "no", "video-zoom" to "0.0000"), MpvOptions.aspect(AspectMode.FIT, 100))
        assertEquals("1.0", MpvOptions.aspect(AspectMode.FILL, 100)["panscan"])
        assertEquals("no", MpvOptions.aspect(AspectMode.STRETCH, 100)["keepaspect"])
        assertEquals("16:9", MpvOptions.aspect(AspectMode.RATIO_16_9, 100)["video-aspect-override"])
        assertEquals("4:3", MpvOptions.aspect(AspectMode.RATIO_4_3, 100)["video-aspect-override"])
        assertEquals("1.0000", MpvOptions.aspect(AspectMode.FIT, 200)["video-zoom"])
        assertEquals("-1.0000", MpvOptions.aspect(AspectMode.FIT, 50)["video-zoom"])
        assertEquals("2.0000", MpvOptions.aspect(AspectMode.FIT, 1000)["video-zoom"], "clamped to 400 %")
    }

    @Test
    fun interpolationOptions() {
        val on = MpvOptions.interpolation(true, 120f)
        assertEquals("display-resample", on["video-sync"])
        assertEquals("oversample", on["tscale"])
        assertEquals("120.000", on["display-fps-override"])
        assertEquals("0", MpvOptions.interpolation(true, null)["display-fps-override"])
        assertEquals(mapOf("interpolation" to "no", "video-sync" to "audio", "display-fps-override" to "0"), MpvOptions.interpolation(false, 60f))
    }

    @Test
    fun casVariantRelaxesWhenAndSetsStrength() {
        val src = "//!HOOK LUMA\n//!WHEN OUTPUT.w OUTPUT.h * LUMA.w LUMA.h * / 1.0 > ! OUTPUT.w OUTPUT.h * LUMA.w LUMA.h * / 1.0 < ! *\n#define SHARPENING 0.0 // x\n"
        val out = ShaderSources.casVariant(src, 0.5)
        assertTrue(out.contains(ShaderSources.CAS_SCALED_WHEN))
        assertFalse(out.contains("1.0 > !"))
        assertTrue(out.contains("#define SHARPENING 0.50 // x"))
        assertEquals("CAS_s050.glsl", ShaderSources.casFileName(0.5))
        assertEquals("CAS_s100.glsl", ShaderSources.casFileName(3.0))
    }

    @Test
    fun bundledCasHasTheLinesTheVariantRewrites() {
        val file = File("src/main/assets/shaders/CAS.glsl").takeIf { it.isFile } ?: return
        val src = file.readText()
        val out = ShaderSources.casVariant(src, 1.0)
        assertEquals(src.lines().count { it.startsWith("//!WHEN") }, out.lines().count { it == ShaderSources.CAS_SCALED_WHEN })
        assertTrue(out.contains("#define SHARPENING 1.00"))
    }

    @Test
    fun installedNamesSeparateFp16DuplicatesAndGenerated() {
        val specs = ShaderCatalog.chain("anime4k-a", app.tatakai.next.model.QualityTier.FAST).map { ShaderSpec(it) } +
            ShaderSpec(ShaderCatalog.CAS, ShaderTransform.CAS, 0.5) + ShaderSpec(ShaderCatalog.FSR)
        val fp32 = ShaderNames.relativePaths(specs, fp16 = false)
        assertEquals("Anime4K_Upscale_CNN_x2_S.glsl", fp32[2])
        assertEquals("Anime4K_Upscale_CNN_x2_S__1.glsl", fp32[4], "second use of the same file gets its own copy")
        assertEquals("gen/CAS_s050.glsl", fp32[5])
        assertEquals("FSR.glsl", fp32[6])
        val fp16 = ShaderNames.relativePaths(specs, fp16 = true)
        assertEquals("${ShaderNames.FP16_DIR}/Anime4K_Clamp_Highlights.glsl", fp16[0])
        assertEquals("FSR.glsl", fp16[6], "FSR is never rewritten")
        assertTrue(ShaderNames.usesFp16(specs, true))
        assertFalse(ShaderNames.usesFp16(listOf(ShaderSpec(ShaderCatalog.CLAMP), ShaderSpec(ShaderCatalog.FSR)), true), "Clamp alone does not count")
        assertFalse(ShaderNames.usesFp16(specs, false))
    }

    @Test
    fun shaderErrorDetection() {
        assertTrue(isShaderCompilationError(20, "vo/gpu/opengl: shader link log (status=0): error"))
        assertTrue(isShaderCompilationError(10, "vo/gpu: GLSL compile failed"))
        assertFalse(isShaderCompilationError(40, "vo/gpu: shader compile failed"), "info level is never evidence")
        assertFalse(isShaderCompilationError(20, "ffmpeg: connection reset"))
    }
}
