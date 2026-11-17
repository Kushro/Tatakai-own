/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

/**
 * R13 frame generation: configuration of one native pipeline, passed to [FgNative.start] and
 * [FgNative.updateConfig].
 *
 * Mirror of `efg::FgConfigN` (same fields, same order). It crosses JNI packed as
 * `ints[9]` + `floats[1]` + `shaderDir` (see [packInts] / [packFloats]); the native side rejects
 * any other length, an out-of-range value or a null with [FgResult.INVALID_ARG].
 *
 * @property multiplier N, 2..5 (Auto is resolved in Kotlin before building the config).
 * @property performanceMode LSFG performance mode.
 * @property flowScale optical-flow scale, finite and > 0.
 * @property inputWidth width of the input surface; must equal [FgNative.createInputSurface]'s.
 * @property inputHeight height of the input surface; must equal [FgNative.createInputSurface]'s.
 * @property outputWidth width of the output (SurfaceView) surface, > 0.
 * @property outputHeight height of the output (SurfaceView) surface, > 0.
 * @property allowFp16 lets the loader use the optional FP16 shader set.
 * @property antiArtifacts enables the 8x8 luma anti-artifact check.
 * @property verbose native verbose logging.
 * @property shaderDir directory holding `fp32/` (and optionally `fp16/`); non-empty.
 */
data class FgConfig(
  val multiplier: Int,
  val performanceMode: Boolean,
  val flowScale: Float,
  val inputWidth: Int,
  val inputHeight: Int,
  val outputWidth: Int,
  val outputHeight: Int,
  val allowFp16: Boolean,
  val antiArtifacts: Boolean,
  val verbose: Boolean,
  val shaderDir: String,
) {
  /** `ints[]` for JNI, indices `kCfg*` of `fg_jni.cpp`. Booleans are 0 | 1. */
  internal fun packInts(): IntArray {
    val ints = IntArray(INT_COUNT)
    ints[IDX_MULTIPLIER] = multiplier
    ints[IDX_PERFORMANCE_MODE] = performanceMode.toInt()
    ints[IDX_INPUT_WIDTH] = inputWidth
    ints[IDX_INPUT_HEIGHT] = inputHeight
    ints[IDX_OUTPUT_WIDTH] = outputWidth
    ints[IDX_OUTPUT_HEIGHT] = outputHeight
    ints[IDX_ALLOW_FP16] = allowFp16.toInt()
    ints[IDX_ANTI_ARTIFACTS] = antiArtifacts.toInt()
    ints[IDX_VERBOSE] = verbose.toInt()
    return ints
  }

  /** `floats[]` for JNI, index `kCfgFlowScale` of `fg_jni.cpp`. */
  internal fun packFloats(): FloatArray {
    val floats = FloatArray(FLOAT_COUNT)
    floats[IDX_FLOW_SCALE] = flowScale
    return floats
  }

  internal companion object {
    // ints[] indices (fg_jni.cpp kCfg*).
    const val IDX_MULTIPLIER = 0
    const val IDX_PERFORMANCE_MODE = 1
    const val IDX_INPUT_WIDTH = 2
    const val IDX_INPUT_HEIGHT = 3
    const val IDX_OUTPUT_WIDTH = 4
    const val IDX_OUTPUT_HEIGHT = 5
    const val IDX_ALLOW_FP16 = 6
    const val IDX_ANTI_ARTIFACTS = 7
    const val IDX_VERBOSE = 8
    const val INT_COUNT = 9

    // floats[] indices (fg_jni.cpp kCfgFlowScale / kCfgFloatCount).
    const val IDX_FLOW_SCALE = 0
    const val FLOAT_COUNT = 1

    /** Native multiplier range (`kMinMultiplier` / `kMaxMultiplier`). */
    const val MIN_MULTIPLIER = 2
    const val MAX_MULTIPLIER = 5

    private fun Boolean.toInt(): Int = if (this) 1 else 0
  }
}
