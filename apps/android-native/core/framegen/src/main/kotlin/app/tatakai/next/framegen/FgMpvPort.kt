/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import android.view.Surface

/**
 * R13: everything [FgController] needs from mpv and from the player SurfaceView, behind one interface so the controller can be
 * driven by a fake in JVM tests.
 *
 * Tatakai: the production implementation is `MpvFrameGenBridge` in :core:player-mpv (this module does not depend on libmpv).
 * Enigmata's raw `attachSurface` / `force-window` / `vo` calls are folded into [connect] / [disconnect], which reuse the
 * engine's own attach / detach sequence (mpv-android order, the engine's `vo` choice and its "surface attached" bookkeeping for
 * loads that wait for a surface). The controller is the only caller and owns the attach/detach bookkeeping (R6): it never
 * calls [disconnect] twice in a row nor [connect] while mpv is connected.
 *
 * All members are called on the main thread.
 */
interface FgMpvPort {
  /**
   * Attaches mpv to [surface] (the SurfaceView's or the FG input), `force-window=yes`, the engine's `vo`, then
   * `android-surface-size=[width]x[height]`.
   */
  fun connect(surface: Surface, width: Int, height: Int)

  /** `vo=null`, `force-window=no`, `detachSurface()`: mpv renders nowhere. */
  fun disconnect()

  /** `setPropertyString(name, value)` (`vid` after a failure). */
  fun setPropertyString(name: String, value: String)

  /** `getPropertyString(name)`, or null when unavailable (FIX-3/m4: `vid` after a failure). */
  fun getPropertyString(name: String): String?

  /** `getPropertyInt(name)`, or null when unavailable (E17: `video-params/dw`, `dh`, `rotate`). */
  fun getPropertyInt(name: String): Int?

  /** `getPropertyDouble(name)`, or null when unavailable (FIX-9/mi-3: `speed`). Default null (treated as speed 1.0). */
  fun getPropertyDouble(name: String): Double? = null

  /** Current mpv `audio-delay` in seconds (effective value), or null when unavailable. */
  fun getAudioDelaySec(): Double?

  /** Writes mpv `audio-delay` in seconds. */
  fun setAudioDelaySec(sec: Double)

  /** mpv `path` of the current file, or null (used to scope I9 to one file). */
  fun currentPath(): String?

  /** The SurfaceView Surface when valid, else null. */
  fun outputSurface(): Surface?

  /** Size of the SurfaceView Surface, or null when unknown / empty. */
  fun outputSize(): Pair<Int, Int>?

  /**
   * Shows or hides the SurfaceView. Hiding it destroys its Surface and showing it creates a new one (SurfaceView ties its
   * Surface to the view visibility); used for R4b (a CPU-presented Surface cannot be reused by EGL).
   */
  fun setOutputVisible(visible: Boolean)

  /** `Surface.setFrameRate(fps)` on the SurfaceView ([alwaysChange] = may switch the display mode, API 31+). */
  fun applyFrameRate(fps: Double, alwaysChange: Boolean)

  /** Removes the frame-rate hint of the SurfaceView. */
  fun clearFrameRate()

  /**
   * Tatakai (replaces Enigmata's `QualityController` state, FIX-5): an upscaler shader chain (Anime4K / FSR, `glsl-shaders`)
   * runs now, so the hybrid input policy gives mpv room for its upscale passes. Default false.
   */
  fun upscalerActive(): Boolean = false
}
