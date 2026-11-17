// SPDX-License-Identifier: AGPL-3.0-or-later
// Ported to Tatakai 0.4.0 (worker A) from Enigmata `app/src/test/java/app/enigmata/domain/anime4k/Anime4KShaderOptimizerTest.kt` (AGPL-3.0-or-later, same author): package renamed, re-indented.
package app.tatakai.next.player.mpv.quality

import java.io.File
import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class Anime4KShaderOptimizerTest {

    private val twoPasses = """
    |// MIT License header stays verbatim
    |// vec4 commented = 1.0;
    |
    |//!DESC plain
    |//!HOOK MAIN
    |//!BIND HOOKED
    |
    |float get_luma(vec4 rgba) {
    |	return dot(vec4(0.299, 0.587, 0.114, 0.0), rgba);
    |}
    |
    |vec4 hook() {
    |	float g = 0.0;
    |	vec4 result = HOOKED_tex(HOOKED_pos);
    |	highp vec2 kept = vec2(0.0);
    |	return result;
    |}
    |
    |//!DESC fsr-like
    |//!HOOK MAIN
    |//!BIND HOOKED
    |vec4 hook() {
    |	float h = uintBitsToFloat(0x3f800000u);
    |	vec3 c = vec3(h);
    |	return vec4(c, 1.0);
    |}
    |""".trimMargin()

    @Test fun `each pass gets its own precision and FP32 markers keep the pass literal`() {
        val out = Anime4KShaderOptimizer.optimize(twoPasses)
        val passes = out.split("//!DESC")
        assertTrue(passes[1].contains("precision mediump float;\nprecision highp int;"))
        assertTrue(passes[1].contains("\tmediump float g = 0.0;"))
        assertTrue(passes[1].contains("\tmediump vec4 result = HOOKED_tex(HOOKED_pos);"))
        assertTrue(passes[1].contains("\thighp vec2 kept = vec2(0.0);"), "already-qualified lines are untouched")
        assertFalse(passes[1].contains("highp float"))
        assertEquals(twoPasses.split("//!DESC")[2], passes[2], "FP32 pass is copied verbatim")
        assertFalse(passes[2].contains("precision"))
        assertFalse(passes[2].contains("mediump"))
        assertFalse(passes[2].contains("highp"))
    }

    @Test fun `optimizer version is 2`() {
        assertEquals(2, Anime4KShaderOptimizer.VERSION)
    }

    private val depthToSpace = """
    |//!DESC conv
    |//!HOOK MAIN
    |//!BIND HOOKED
    |// a comment with [idx] does not count
    |vec4 hook() {
    |    float w[4];
    |    w[0] = HOOKED_tex(HOOKED_pos)[ 1 ];
    |    vec4 result = vec4(w[0]);
    |    return result;
    |}
    |
    |//!DESC Depth-to-Space
    |//!HOOK MAIN
    |//!BIND conv2d_last_tf
    |vec4 hook() {
    |    vec2 f0 = fract(conv2d_last_tf_pos * conv2d_last_tf_size);
    |    ivec2 i0 = ivec2(f0 * vec2(2.0));
    |    float c0 = conv2d_last_tf_tex((vec2(0.5) - f0) * conv2d_last_tf_pt + conv2d_last_tf_pos)[i0.y * 2 + i0.x];
    |    return vec4(c0, c0, c0, 1.0);
    |}
    |""".trimMargin()

    @Test fun `dynamic subscript keeps the pass literal and literal indices do not`() {
        val out = Anime4KShaderOptimizer.optimize(depthToSpace)
        val src = depthToSpace.split("//!DESC")
        val passes = out.split("//!DESC")
        assertEquals(src.size, passes.size)
        assertTrue(passes[1].contains("precision mediump float;\nprecision highp int;"))
        assertTrue(passes[1].contains("    mediump float w[4];"))
        assertTrue(passes[1].contains("    mediump vec4 result = vec4(w[0]);"))
        assertEquals(src[2], passes[2], "Depth-to-Space pass is copied verbatim")
        assertFalse(passes[2].contains("precision"))
        assertFalse(passes[2].contains("mediump"))
    }

    @Test fun `passNeedsHighpFloat detects dynamic subscripts only in code`() {
        fun needs(vararg body: String) = Anime4KShaderOptimizer.passNeedsHighpFloat(body.toList(), 0)
        assertTrue(needs("float c = v[i];"))
        assertTrue(needs("float c = tex(p)[i0.y * 2 + i0.x];"))
        assertTrue(needs("float c = m[ idx ][0];"))
        assertFalse(needs("float w[4];", "w[0] = v[ 1 ];", "float u[];"))
        assertFalse(needs("float c = v[0]; // v[i] in a comment"))
        assertFalse(needs("float c = 1.0;", "//!DESC next[i]", "float d = v[i];"), "scan stops at the next header")
    }

    @Test fun `function signatures comments and headers are intact`() {
        val out = Anime4KShaderOptimizer.optimize(twoPasses)
        assertTrue(out.startsWith("// MIT License header stays verbatim\n// vec4 commented = 1.0;\n"))
        assertTrue(out.contains("\nfloat get_luma(vec4 rgba) {\n"))
        assertEquals(2, Regex("\nvec4 hook\\(\\) \\{\n").findAll(out).count())
        assertEquals(
            twoPasses.lines().filter { it.startsWith("//!") },
            out.lines().filter { it.startsWith("//!") },
        )
    }

    @Test fun `every FP32 marker keeps the pass literal`() {
        for (marker in Anime4KShaderOptimizer.FP32_MARKERS) {
            val src = "//!HOOK MAIN\nvec4 hook() {\n\tfloat x = $marker(1.0);\n\treturn vec4(x);\n}\n"
            assertEquals(src, Anime4KShaderOptimizer.optimize(src), marker)
        }
    }

    @Test fun `allowlist only accepts Anime4K files`() {
        assertTrue(Anime4KShaderOptimizer.shouldOptimize("Anime4K_Restore_CNN_S.glsl"))
        assertTrue(Anime4KShaderOptimizer.shouldOptimize("Anime4K_Darken_HQ.glsl"))
        for (name in listOf("FSRCNNX_x2_8-0-4-1.glsl", "FSRCNNX_x2_8-0-4-1_LineArt.glsl", "SSimSuperRes.glsl",
            "SSimDownscaler.glsl", "Ani4Kv2_ArtCNN_C4F32_i2_CMP.glsl", "ArtCNN_C4F16.glsl", "CAS.glsl", "anime4k_lower.glsl")) {
            assertFalse(Anime4KShaderOptimizer.shouldOptimize(name), name)
        }
    }

    private val crelu = """
    |//!DESC conv
    |//!HOOK MAIN
    |//!BIND conv2d_tf
    |#define go_0(x_off, y_off) (max((conv2d_tf_texOff(vec2(x_off, y_off))), 0.0))
    |#define go_1(x_off, y_off) (max(-(conv2d_tf_texOff(vec2(x_off, y_off))), 0.0))
    |vec4 hook() {
    |    vec4 result = mat4(1.0) * go_0(-1.0, -1.0);
    |    result += mat4(1.0) * go_1(1.0, 0.0);
    |    return result;
    |}
    |""".trimMargin()

    @Test fun `CRELU macros become single texel fetches`() {
        val out = Anime4KShaderOptimizer.optimize(crelu)
        assertFalse(out.contains("go_0("))
        assertFalse(out.contains("go_1("))
        assertTrue(out.contains("vec4 t_m1_m1 = conv2d_tf_texOff(vec2(-1.0, -1.0));"))
        assertTrue(out.contains("vec4 t_1_1 = conv2d_tf_texOff(vec2(1.0, 1.0));"))
        assertTrue(out.contains("max(t_m1_m1, 0.0)"))
        assertTrue(out.contains("max(-t_1_0, 0.0)"))
        assertEquals(9, Regex("_texOff\\(").findAll(out).count())
    }

    @Test fun `unknown offsets keep the original CRELU pass`() {
        val wide = crelu.replace("go_1(1.0, 0.0)", "go_1(2.0, 0.0)")
        val out = Anime4KShaderOptimizer.optimize(wide)
        assertTrue(out.contains("#define go_0("))
        assertTrue(out.contains("go_1(2.0, 0.0)"))
        assertFalse(out.contains("t_m1_m1"))
        assertFalse(out.contains("_unknown"))
    }

    @Test fun `bundled Anime4K shader keeps its structure`() {
        val file = listOf("src/main/assets/shaders", "app/src/main/assets/shaders")
            .map { File(it, "Anime4K_Restore_CNN_S.glsl") }.firstOrNull { it.isFile } ?: return
        val src = file.readText()
        val out = Anime4KShaderOptimizer.optimize(src)
        val headers = { s: String -> s.lines().map { it.trim() }.filter { it.startsWith("//!") } }
        assertEquals(headers(src), headers(out))
        assertEquals(Regex("vec4 hook\\(\\)").findAll(src).count(), Regex("vec4 hook\\(\\)").findAll(out).count())
        assertEquals(src.lines().count { it.trim().startsWith("//!HOOK") },
            Regex("precision (mediump|highp) float;").findAll(out).count())
        assertFalse(out.contains("highp float;"), "Anime4K CNN passes have no FP32 markers")
        assertFalse(Regex("\\bgo_[01]\\(\\s*-?\\d").containsMatchIn(out.substringAfter("conv2d_1_tf")))
    }

    private fun shaderAssetsDir(): File? =
        listOf("src/main/assets/shaders", "app/src/main/assets/shaders").map { File(it) }.firstOrNull { it.isDirectory }

    private val dynamicSubscript = Regex("""\[\s*[^\]\s0-9][^\]]*\]""")

    /** Splits at each pass header; the bundled Anime4K files start every header block with `//!DESC`. */
    private fun passes(text: String): List<String> = text.split(Regex("(?=//!DESC)"))

    private fun hasDynamicSubscript(pass: String): Boolean =
        pass.lines().filterNot { it.trim().startsWith("//") }.any { dynamicSubscript.containsMatchIn(it.substringBefore("//")) }

    @Test fun `bundled passes with dynamic subscripts are copied byte for byte`() {
        val dir = shaderAssetsDir() ?: return
        val files = dir.listFiles { f -> f.isFile && f.name.startsWith("Anime4K_") && f.name.endsWith(".glsl") }!!
            .sortedBy { it.name }
        var literalPasses = 0
        val depthToSpaceFiles = mutableListOf<String>()
        for (file in files) {
            // The optimizer emits `\n` line endings; compare against the LF form (what CI checks out).
            val src = file.readText().replace("\r\n", "\n")
            val out = Anime4KShaderOptimizer.optimize(src)
            val srcPasses = passes(src)
            val outPasses = passes(out)
            assertEquals(srcPasses.size, outPasses.size, file.name)
            for (k in srcPasses.indices) {
                if (!hasDynamicSubscript(srcPasses[k])) continue
                assertEquals(srcPasses[k], outPasses[k], "${file.name} pass $k must be literal")
                literalPasses++
                if (srcPasses[k].contains("Depth-to-Space")) depthToSpaceFiles += file.name
            }
        }
        if (files.isEmpty()) return
        for (name in listOf("Anime4K_Upscale_CNN_x2_S.glsl", "Anime4K_Upscale_CNN_x2_M.glsl", "Anime4K_Upscale_CNN_x2_L.glsl",
            "Anime4K_Upscale_Denoise_CNN_x2_S.glsl", "Anime4K_Upscale_Denoise_CNN_x2_M.glsl", "Anime4K_Upscale_Denoise_CNN_x2_L.glsl")) {
            if (files.any { it.name == name }) assertTrue(name in depthToSpaceFiles, "$name Depth-to-Space pass stays literal")
        }
        assertTrue(literalPasses >= depthToSpaceFiles.size)
    }

    @Test fun `bundled CNN passes without index are still rewritten`() {
        val file = shaderAssetsDir()?.let { File(it, "Anime4K_Upscale_CNN_x2_S.glsl") }?.takeIf { it.isFile } ?: return
        val src = file.readText().replace("\r\n", "\n")
        val out = Anime4KShaderOptimizer.optimize(src)
        val srcPasses = passes(src)
        val outPasses = passes(out)
        val rewritable = srcPasses.indices.filter { k -> srcPasses[k].startsWith("//!DESC") && !hasDynamicSubscript(srcPasses[k]) }
        assertTrue(rewritable.isNotEmpty())
        for (k in rewritable) {
            assertTrue(outPasses[k].contains("precision mediump float;\nprecision highp int;"), "pass $k")
            assertTrue(outPasses[k].contains("mediump vec4"), "pass $k")
        }
        assertEquals(rewritable.size, Regex("precision mediump float;").findAll(out).count())
        assertFalse(out.contains("highp float;"))
        // C.R.E.L.U. still applies to the convolution passes.
        assertTrue(out.contains("// optimized go_0 macro"))
    }
}
