/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

// Ported to Tatakai 0.4.0 (worker A) from Enigmata `app/src/main/java/app/enigmata/domain/anime4k/Anime4KShaderOptimizer.kt`
// (AGPL-3.0-or-later, same author): package renamed and re-indented; logic unchanged.

package app.tatakai.next.player.mpv.quality

/**
 * V1. Pure FP16 (mediump) rewrite of bundled Anime4K GLSL, ported from mpvRx's
 * `Anime4KManager.optimizeShaderContent` (domain/anime4k/Anime4KManager.kt there).
 *
 * 1. Each pass gets a default `precision mediump float; precision highp int;` before its first
 *    code line, and every unqualified float-type *variable declaration* in its body is rewritten
 *    to `mediump <type> ...`. Some mobile drivers (notably Adreno) promote a default mediump back
 *    to highp for mat4-heavy code; explicit qualifiers cannot be overridden.
 * 2. Passes that need full precision ([passNeedsHighpFloat]) are copied verbatim: no injected
 *    `precision`, no added qualifiers, no C.R.E.L.U. rewrite (mpv's default is highp). That covers
 *    FP32-only constructs ([FP32_MARKERS]: FSR EASU/RCAS, bitcasts, bitfield ops, fast reciprocal
 *    tricks - FP16 turns them into garbage) and dynamically indexed components such as the
 *    Depth-to-Space `tex(...)[i0.y * 2 + i0.x]`, which Adreno fails to link when stored mediump.
 * 3. C.R.E.L.U. passes (`go_0`/`go_1` macros over one texture) fetch each 3x3 texel once instead
 *    of twice. Any offset outside {-1,0,1}, a missing `vec4 hook() {`, or mismatched textures
 *    leaves that pass untouched.
 *
 * Function signatures, comments, preprocessor lines, already-qualified declarations and
 * constructor-led lines are never touched. Only files accepted by [shouldOptimize] are
 * transformed (allowlist `Anime4K_`): FSRCNNX/SSim/ArtCNN/user shaders are copied verbatim.
 * Output always uses `\n` line endings. Bump [VERSION] whenever the output changes so installed copies are regenerated.
 */
object Anime4KShaderOptimizer {
    /** 2: passes with dynamic subscripts or [FP32_MARKERS] are emitted verbatim. */
    const val VERSION = 2

    /** GLSL tokens that require FP32 (highp) floats. A pass containing any of them is kept verbatim. */
    val FP32_MARKERS: List<String> = listOf(
        "floatBitsToUint",
        "uintBitsToFloat",
        "floatBitsToInt",
        "intBitsToFloat",
        "bitfieldExtract",
        "bitfieldInsert",
        "FsrEasu",
        "FsrRcas",
        "APrxLoRcpF1",
        "APrxLoRsqF1",
        "APrxMedRcpF1",
    )

    private val FLOAT_TYPES = listOf("mat4", "mat3", "mat2", "vec4", "vec3", "vec2", "float")

    fun shouldOptimize(fileName: String): Boolean = fileName.startsWith("Anime4K_")

    fun optimize(src: String): String {
        val lines = src.lines()
        // One entry per pass (a `//!` header block plus its body; the preamble is the first entry).
        val segments = ArrayList<String>()
        val current = ArrayList<String>()
        var inHeader = false
        var pendingInjection = false
        var injected = false
        var literal = false

        fun flushPass() {
            if (current.isEmpty()) return
            val text = current.joinToString("\n")
            segments += if (literal) text else optimizeCreluPasses(text)
            current.clear()
        }

        for (i in lines.indices) {
            val line = lines[i]
            val trimmed = line.trim()
            if (trimmed.startsWith("//!")) {
                if (!inHeader) {
                    flushPass()
                    inHeader = true
                    injected = false
                    literal = false
                }
                current += line
                continue
            }
            if (inHeader) {
                inHeader = false
                pendingInjection = true
            }
            if (pendingInjection && trimmed.isNotEmpty() && !trimmed.startsWith("//")) {
                pendingInjection = false
                if (passNeedsHighpFloat(lines, i)) {
                    // Full-precision pass: copied verbatim (mpv's default highp), no C.R.E.L.U. rewrite.
                    literal = true
                } else {
                    current += "precision mediump float;"
                    current += "precision highp int;"
                    injected = true
                }
            }
            current += if (injected) rewriteLineWithExplicitPrecision(line, "mediump") else line
        }
        flushPass()
        return segments.joinToString("\n")
    }

    /**
   * A subscript whose content is not only digits (`[i0.y * 2 + i0.x]`, `[idx]`). Literal indices
   * (`[0]`, `[ 4 ]`) and unsized arrays (`[]`) do not match.
   */
    private val DYNAMIC_SUBSCRIPT = Regex("""\[\s*[^\]\s0-9][^\]]*\]""")

    /**
   * True if the pass body starting at [bodyStart] (until the next `//!` header) must keep full
   * precision: it contains an [FP32_MARKERS] token, or (outside `//` comments) a dynamically
   * indexed vector/array component. Adreno (GLES 3.2) fails to link programs that assign such a
   * component to a mediump variable, e.g. Anime4K's Depth-to-Space `tex(...)[i0.y * 2 + i0.x]`.
   */
    internal fun passNeedsHighpFloat(lines: List<String>, bodyStart: Int): Boolean {
        var j = bodyStart
        while (j < lines.size && !lines[j].trim().startsWith("//!")) {
            val line = lines[j]
            if (FP32_MARKERS.any { line.contains(it) }) return true
            if (DYNAMIC_SUBSCRIPT.containsMatchIn(line.substringBefore("//"))) return true
            j++
        }
        return false
    }

    /**
   * `vec4 result = ...` -> `mediump vec4 result = ...`. Unchanged: blank lines, comments,
   * preprocessor lines, already-qualified lines, function definitions/prototypes
   * (`vec4 hook() {`, `float f(float x)`) and constructor-led lines (`vec4(...)`).
   */
    internal fun rewriteLineWithExplicitPrecision(line: String, precision: String): String {
        val trimmed = line.trim()
        if (trimmed.isEmpty() || trimmed.startsWith("//") || trimmed.startsWith("#")) return line
        if (trimmed.startsWith("mediump ") || trimmed.startsWith("highp ") || trimmed.startsWith("lowp ")) return line
        val indent = line.substring(0, line.length - line.trimStart().length)
        for (type in FLOAT_TYPES) {
            if (!trimmed.startsWith("$type ") && !trimmed.startsWith("$type\t")) continue
            val afterType = trimmed.substring(type.length).trimStart()
            if (afterType.isEmpty() || !(afterType.first().isLetter() || afterType.first() == '_')) return line
            val isFunction = afterType.contains("(") && !afterType.substringBefore("(").contains("=")
            if (isFunction) return line
            return "$indent$precision $trimmed"
        }
        return line
    }

    private fun optimizeCreluPasses(content: String): String =
        content.split(PASS_SPLIT).joinToString("") { optimizeSinglePass(it) }

    private val PASS_SPLIT = Regex("(?=(?://!DESC|//!HOOK))")
    private val GO0_DEFINE =
        Regex("""#define\s+go_0\([^)]+\)\s+\(max\(\(?\s*([A-Za-z0-9_]+)_texOff\(vec2\([^)]+\)\)\)?\s*,\s*0\.0\)\)""")
    private val GO1_DEFINE =
        Regex("""#define\s+go_1\([^)]+\)\s+\(max\(-\(?\s*([A-Za-z0-9_]+)_texOff\(vec2\([^)]+\)\)\)?\s*,\s*0\.0\)\)""")
    private val HOOK_START = Regex("""vec4\s+hook\(\s*\)\s*\{""")
    private val GO0_CALL = Regex("""go_0\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)""")
    private val GO1_CALL = Regex("""go_1\(\s*(-?\d+(?:\.\d+)?)\s*,\s*(-?\d+(?:\.\d+)?)\s*\)""")

    internal fun optimizeSinglePass(pass: String): String {
        val match0 = GO0_DEFINE.find(pass) ?: return pass
        val match1 = GO1_DEFINE.find(pass) ?: return pass
        val tex = match0.groupValues[1]
        if (tex != match1.groupValues[1] || !HOOK_START.containsMatchIn(pass)) return pass

        var optimized = pass
            .replace(match0.value, "// optimized go_0 macro")
            .replace(match1.value, "// optimized go_1 macro")
        val fetches = buildString {
            append("vec4 hook() {\n")
            for (x in OFFSETS) for (y in OFFSETS) {
                append("    vec4 t_${x.second}_${y.second} = ${tex}_texOff(vec2(${x.first}, ${y.first}));\n")
            }
        }.trimEnd('\n')
        optimized = optimized.replaceFirst(HOOK_START, Regex.escapeReplacement(fetches))
        optimized = GO0_CALL.replace(optimized) {
            "max(t_${mapCoord(it.groupValues[1])}_${mapCoord(it.groupValues[2])}, 0.0)"
        }
        optimized = GO1_CALL.replace(optimized) {
            "max(-t_${mapCoord(it.groupValues[1])}_${mapCoord(it.groupValues[2])}, 0.0)"
        }
        // An offset outside {-1,0,1} would reference an undeclared texel: keep the original pass.
        if (optimized.contains("_unknown")) return pass
        return optimized
    }

    private val OFFSETS = listOf("-1.0" to "m1", "0.0" to "0", "1.0" to "1")

    private fun mapCoord(c: String): String =
        when (c.trim()) {
            "-1.0", "-1" -> "m1"
            "0.0", "0" -> "0"
            "1.0", "1" -> "1"
            else -> "unknown"
        }
}
