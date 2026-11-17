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
 * R13 frame generation: result codes returned by [FgNative].
 *
 * Mirror of `efg::FgResult` in `cpp/fg/fg_types.h`; the values cross JNI unchanged and must stay in
 * sync with it. `extractShaders` returns a count (>= 0) or one of the negative codes, so callers
 * resolve a code with [fromCode] only when it is negative.
 *
 * - [VK_INIT] / [MISSING_EXTENSION]: `FgDevice::create`.
 * - [LSFG_INIT] / [CONTEXT] / [SHADERS_MISSING]: `FgEngine::start` (CONTEXT is also any internal
 *   exception of `start` / `updateConfig`).
 * - [DEVICE_LOST]: only appears in [FgStats.lastError] (state FAILED, invariant I9).
 * - [DLL_UNREADABLE] .. [WRITE_FAILED]: `extractShaders`.
 */
enum class FgResult(val code: Int) {
  OK(0),
  NOT_INITIALIZED(-1),
  VK_INIT(-2),
  MISSING_EXTENSION(-3),
  SHADERS_MISSING(-4),
  LSFG_INIT(-5),
  CONTEXT(-6),
  READER(-7),
  SURFACE(-8),
  DEVICE_LOST(-9),
  BUSY(-10),
  INVALID_ARG(-11),
  DLL_UNREADABLE(-20),
  MISSING_SPIRV(-21),
  BINDING_REWRITE(-22),
  WRITE_FAILED(-23),
  ;

  companion object {
    private val byCode: Map<Int, FgResult> = entries.associateBy { it.code }

    /** The enum constant for [code], or null when the code is unknown (or a count >= 1). */
    fun fromCode(code: Int): FgResult? = byCode[code]
  }
}
