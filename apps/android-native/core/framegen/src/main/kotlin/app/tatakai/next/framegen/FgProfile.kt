/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import app.tatakai.next.model.FrameGenProfile

/**
 * R13: frame generation performance profile (fact 25), selectable in Settings and in the player. Tatakai: the enum lives in
 * :core:model ([FrameGenProfile], persisted in `AppSettings.frameGen.profile`, default BALANCED); this alias keeps the ported
 * Enigmata code and tests unchanged.
 *
 * - SAVER: performance mode, flow scale 0.5, input at 75 % of the output Surface.
 * - BALANCED: performance mode, flow scale 0.5, full-resolution input (default).
 * - QUALITY: quality mode (LSFG_3_1), flow scale 1.0, full-resolution input.
 * - CUSTOM: every value set by hand (`FrameGenSettings.custom*`); it has no preset.
 */
typealias FgProfile = FrameGenProfile

/** Fixed tuning of this profile, or null for CUSTOM (its values come from the user). */
fun FgProfile.presetTuning(): FgTuning? =
  when (this) {
    FgProfile.SAVER -> FgTuning.SAVER
    FgProfile.BALANCED -> FgTuning.BALANCED
    FgProfile.QUALITY -> FgTuning.QUALITY
    FgProfile.CUSTOM -> null
  }

/**
 * R13: the LSFG knobs a profile controls.
 *
 * @property performanceMode true = LSFG performance mode; false = quality mode (LSFG_3_1).
 * @property flowScale optical-flow scale passed to the engine.
 * @property inputScale size of the AImageReader input relative to the output Surface.
 *
 * Valid ranges for [FgProfile.CUSTOM] (see [isValidCustom]): [flowScale] in
 * [FLOW_SCALE_MIN]..[FLOW_SCALE_MAX] (0.25..1.0) and [inputScale] one of [INPUT_SCALES]
 * (75 % or 100 %, the same choices as the presets). The presets are always valid.
 */
data class FgTuning(
  val performanceMode: Boolean,
  val flowScale: Float,
  val inputScale: Float,
) {
  /** True when every value is inside the ranges accepted for [FgProfile.CUSTOM]. */
  fun isValidCustom(): Boolean = flowScale in FLOW_SCALE_MIN..FLOW_SCALE_MAX && inputScale in INPUT_SCALES

  companion object {
    const val FLOW_SCALE_MIN = 0.25f
    const val FLOW_SCALE_MAX = 1.0f

    /** Allowed input scales: 75 % or 100 % of the output Surface. */
    val INPUT_SCALES: List<Float> = listOf(0.75f, 1.0f)

    /** Preset of [FgProfile.SAVER]. */
    val SAVER = FgTuning(performanceMode = true, flowScale = 0.5f, inputScale = 0.75f)

    /** Preset of [FgProfile.BALANCED]. */
    val BALANCED = FgTuning(performanceMode = true, flowScale = 0.5f, inputScale = 1.0f)

    /** Preset of [FgProfile.QUALITY]. */
    val QUALITY = FgTuning(performanceMode = false, flowScale = 1.0f, inputScale = 1.0f)
  }
}
