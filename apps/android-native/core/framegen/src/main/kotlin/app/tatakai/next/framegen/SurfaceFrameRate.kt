/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import android.os.Build
import android.view.Surface

/**
 * A5b / R13 (Enigmata `ui/player/SurfaceFrameRate.kt`). Refresh-rate matching: hints the display compositor (via
 * [Surface.setFrameRate]) that the player surface shows content at a given frame rate, so the system can pick a compatible
 * display mode (e.g. 24 fps x 5 -> 120 Hz while FG runs).
 *
 * - Below API 30 every call is a no-op (the API does not exist).
 * - API 30 uses `setFrameRate(fps, FRAME_RATE_COMPATIBILITY_FIXED_SOURCE)`; that platform version has no seamlessness parameter.
 * - API 31+ passes `CHANGE_FRAME_RATE_ONLY_IF_SEAMLESS`, or `CHANGE_FRAME_RATE_ALWAYS` with `alwaysChange = true` (FG active),
 *   which allows a non-seamless mode switch.
 *
 * All calls are wrapped in [runCatching]: a released/abandoned surface throws IllegalArgumentException/IllegalStateException and
 * that must never crash playback.
 */
object SurfaceFrameRate {
  /** Upper bound for a plausible frame rate; larger values are treated as garbage. */
  internal const val MAX_FPS = 1000.0

  /** Pure part: the frame rate to hint, or null when [fps] is not usable (null, NaN, infinite, <= 0 or implausibly large). */
  internal fun normalizeFps(fps: Double?): Float? {
    if (fps == null || !fps.isFinite() || fps <= 0.0 || fps > MAX_FPS) return null
    val value = fps.toFloat()
    return value.takeIf { it.isFinite() && it > 0f }
  }

  /** Hints [surface] to run at [fps]. No-op below API 30, for a null/invalid surface or fps. */
  fun apply(surface: Surface?, fps: Double?, alwaysChange: Boolean = false) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
    if (surface == null || !surface.isValid) return
    val rate = normalizeFps(fps) ?: return
    runCatching {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        surface.setFrameRate(
          rate,
          Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE,
          if (alwaysChange) Surface.CHANGE_FRAME_RATE_ALWAYS else Surface.CHANGE_FRAME_RATE_ONLY_IF_SEAMLESS,
        )
      } else {
        surface.setFrameRate(rate, Surface.FRAME_RATE_COMPATIBILITY_FIXED_SOURCE)
      }
    }
  }

  /** Removes any frame-rate hint from [surface] (rate 0 = no preference). No-op below API 30. */
  fun clear(surface: Surface?) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return
    if (surface == null || !surface.isValid) return
    runCatching {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        surface.setFrameRate(0f, Surface.FRAME_RATE_COMPATIBILITY_DEFAULT, Surface.CHANGE_FRAME_RATE_ONLY_IF_SEAMLESS)
      } else {
        surface.setFrameRate(0f, Surface.FRAME_RATE_COMPATIBILITY_DEFAULT)
      }
    }
  }
}
