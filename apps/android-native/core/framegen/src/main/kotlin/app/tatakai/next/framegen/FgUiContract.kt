/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import android.net.Uri
import app.tatakai.next.model.FrameGenSettings
import kotlinx.coroutines.flow.StateFlow

/**
 * R13: why frame generation cannot run right now. The visible text (es / en) lives in this module's
 * `res/values{,-es}/strings_frame_gen.xml` (keys `fg_reason_*`, see [FrameGenText.reason]); this enum carries no UI text.
 *
 * - [Kind.INELIGIBLE]: the device or install can never run FG until something changes outside the
 *   player (ABI, library, Vulkan, shaders).
 * - [Kind.SUSPENDED]: FG is paused by the current situation and comes back by itself when it ends
 *   (Tatakai Link remote mode, PiP, background, Eco/low power, 8K, HDR passthrough).
 * - [Kind.FAILED]: the pipeline failed for this file (I9); no retry until another file loads.
 */
enum class FgUnavailableReason(val kind: Kind) {
  /** The APK ABI has no `libtatakai_fg.so` (only arm64-v8a and x86_64). */
  ABI(Kind.INELIGIBLE),

  /** The library failed to load or register ([FgNative.isAvailable] is false). */
  LIBRARY(Kind.INELIGIBLE),

  /**
   * Vulkan is below the required level (1.3, or 1.1 with `allowUntestedVulkan`, Amendment 1) or a
   * required extension (AHB import, swapchain) is missing.
   */
  VULKAN(Kind.INELIGIBLE),

  /** The LSFG shaders have not been imported (or the set is incomplete). */
  SHADERS(Kind.INELIGIBLE),

  /** Picture-in-picture. */
  PIP(Kind.SUSPENDED),

  /** The player is in the background. */
  BACKGROUND(Kind.SUSPENDED),

  /** Low-power (Eco / battery saver) mode. */
  LOW_POWER(Kind.SUSPENDED),

  /** 8K suspension. */
  EIGHT_K(Kind.SUSPENDED),

  /** HDR passthrough output. */
  HDR_PASSTHROUGH(Kind.SUSPENDED),

  /** The Vulkan device was lost or the pipeline failed (state FAILED). */
  DEVICE_LOST(Kind.FAILED),

  /**
   * Amendment 1 (REV 10): the native pipeline failed to start for a reason other than a lost
   * device (input surface, reader, surface, busy, context, LSFG init...).
   */
  START_FAILED(Kind.FAILED),

  /**
   * Tatakai Link: the paired PC already processes this video and the phone must not generate frames on top (Link «PC» mode,
   * I4). Set through [FrameGenController.onRemote].
   */
  REMOTE(Kind.SUSPENDED),
  ;

  /** Category of a [FgUnavailableReason]. */
  enum class Kind { INELIGIBLE, SUSPENDED, FAILED }
}

/**
 * R13: state exposed to the player UI (chip, FG panel, stats overlay) and to Settings.
 *
 * @property enabled the user switch (persisted in `AppSettings.frameGen.enabled`).
 * @property active the native pipeline is presenting right now.
 * @property multiplier the effective N (2..5) while [active]; with [auto] it is the Auto choice. Otherwise the preference
 *   (0 = Auto).
 * @property auto the multiplier preference is Auto (0).
 * @property unavailable why FG cannot run, or null when it can (or when it is not known yet: see [eligibilityKnown]).
 * @property experimental the GPU is outside the tested range (only Adreno 7xx+ is not).
 * @property profile the selected performance profile.
 * @property stats Amendment 2 (E9): the last stats snapshot while active **or after a failure**; null otherwise.
 * @property noInputTimeout Amendment 2 (E9): the current failure came from the «no input frames» watchdog.
 * @property audioCompMs FG audio compensation currently inside mpv's audio-delay (latency L + manual offset), in ms.
 * @property refreshHz refresh rate of the display's physical mode, refreshed on every stats reading; 0 when unknown.
 * @property overloaded Amendment 3 (E19): the GPU cannot keep up (measured input below 80 % of the source for > 5 s).
 * @property measuredInFps Amendment 3 (E19): smoothed measured FG input rate, in fps; 0 when unknown.
 * @property sourceFps Amendment 3 (E19): nominal frame rate of the source, in fps; 0 when unknown.
 * @property liveFps Tatakai: input / output fps measured between the last two stats readings while active (null = no
 *   measurement yet). `FgStatsView.from(state, state.liveFps)` gives the rounded numbers for an overlay.
 * @property bound Tatakai: an MPV session is bound (FG only exists on the MPV engine). Settings show the state without it.
 * @property eligibilityKnown Tatakai: the expensive facts (library, shaders) were probed; until then [unavailable] only
 *   carries the cheap verdicts (ABI, declared Vulkan, situation). [FrameGenController.refresh] probes them.
 * @property shadersImported Tatakai: the LSFG shaders extracted from the user's Lossless.dll are installed (null = unknown).
 * @property shadersImportedAt Tatakai: ISO-8601 instant of the last import (manifest), or null.
 * @property settings Tatakai: the current [FrameGenSettings] (match refresh, audio offset, stats switch...).
 */
data class FrameGenUiState(
  val enabled: Boolean = false,
  val active: Boolean = false,
  val multiplier: Int = 0,
  val auto: Boolean = true,
  val unavailable: FgUnavailableReason? = null,
  val experimental: Boolean = false,
  val profile: FgProfile = FgProfile.BALANCED,
  val stats: FgStats? = null,
  val noInputTimeout: Boolean = false,
  val audioCompMs: Double = 0.0,
  val refreshHz: Float = 0f,
  val overloaded: Boolean = false,
  val measuredInFps: Double = 0.0,
  val sourceFps: Double = 0.0,
  val liveFps: FgFps? = null,
  val bound: Boolean = false,
  val eligibilityKnown: Boolean = false,
  val shadersImported: Boolean? = null,
  val shadersImportedAt: String? = null,
  val settings: FrameGenSettings = FrameGenSettings(),
)

/** Enigmata's name of [FrameGenUiState] (ported code and tests). */
typealias FgUiState = FrameGenUiState

/** The multiplier choices: Auto (by the display refresh and the GPU budget) or a fixed 2x..5x. */
enum class FrameGenMultiplier(val n: Int) {
  AUTO(0),
  X2(2),
  X3(3),
  X4(4),
  X5(5),
  ;

  companion object {
    /** The choice for a stored value (0 = Auto, 2..5); anything else is Auto. */
    fun of(n: Int): FrameGenMultiplier = entries.firstOrNull { it.n == n } ?: AUTO
  }
}

/**
 * Public API of the phone frame generation for the player UI (worker D), Tatakai Link (worker C) and Settings. One app-wide
 * instance (Koin `single`, `koinInject<FrameGenController>()`); the MPV session binds itself to it while it plays, so the
 * state always describes the current MPV playback. Main thread only.
 */
interface FrameGenController {
  val state: StateFlow<FrameGenUiState>

  /** Turns FG on or off hot, without reloading the file; persisted. Off -> on only when eligible (else [state] says why). */
  fun setEnabled(enabled: Boolean)

  /** Auto or 2..5; global, applied hot. */
  fun setMultiplier(multiplier: FrameGenMultiplier)

  /** SAVER / BALANCED / QUALITY (CUSTOM uses [setCustomTuning]); global, applied hot. */
  fun setProfile(profile: FgProfile)

  /** Values used by [FgProfile.CUSTOM] (bounded). */
  fun setCustomTuning(tuning: FgTuning)

  /** Manual audio compensation offset in ms (-500..+500), global; recomposes `audio-delay` while active. */
  fun setAudioOffsetMs(ms: Int)

  /** «Change the display refresh for FG» (fps x N with `Surface.setFrameRate`). */
  fun setMatchRefresh(enabled: Boolean)

  /** «Allow FG on untested GPUs (experimental)»: Vulkan 1.1 / 1.2 become eligible. */
  fun setAllowUntestedGpu(allowed: Boolean)

  /** «Show FG stats» (overlay switch for the player UI, plus a logcat line about once per second). */
  fun setShowStats(show: Boolean)

  /**
   * Tatakai Link: the PC processes the current video ([active]). With [phoneFgAllowed] false (Link «PC», or a PC profile that
   * already generates frames) FG is suspended with [FgUnavailableReason.REMOTE]; true («Mixto»: the PC upscales, the phone
   * generates frames) lets FG run on the PC's stream like on a local file. Never changes the user switch; leaving remote mode
   * resumes FG when the user had it on.
   */
  fun onRemote(active: Boolean, phoneFgAllowed: Boolean = false)

  /** Picture-in-picture suspends FG (fact 16); the player UI reports the PiP mode here. */
  fun onPip(inPip: Boolean)

  /** Eco / low power and 8K suspend FG (fact 16). Battery saver and 8K video are also detected by the controller itself. */
  fun onLowPower(lowPower: Boolean, eightK: Boolean = false)

  /** HDR passthrough output suspends FG (fact 16). */
  fun onHdrPassthrough(passthrough: Boolean)

  /** Probes the expensive eligibility facts (library, shaders) off the main thread; call it when the FG state is shown. */
  fun refresh()

  /** Imports the user's `Lossless.dll` behind [uri] (SAF): extracts the SPIR-V shaders into app storage. Stops FG first. */
  suspend fun importShaders(uri: Uri): InstallResult

  /** Removes the imported shaders (FG becomes ineligible with [FgUnavailableReason.SHADERS]). Stops FG first. */
  suspend fun removeShaders(): Boolean

  /** User part of an effective `audio-delay` (what an audio-delay control shows and saves, I6). */
  fun userAudioDelaySec(effective: Double): Double

  /** Effective `audio-delay` for a user value (user + FG compensation while active). */
  fun effectiveAudioDelaySec(user: Double): Double
}
