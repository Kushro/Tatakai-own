/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import java.util.Locale
import kotlin.math.round

/**
 * Amendment 2 (E11): what the FG stats overlay, the FrameGen panel card and the logcat line show.
 * Pure (no Android, no resources): the UI maps each value to its own strings.
 */
enum class FgStatsStatus { ACTIVE, PAUSED, SUSPENDED, FAILED, OFF }

/** Amendment 2 (E11): input and output frame rates measured by [FgFpsMeter], in fps. */
data class FgFps(
  val inFps: Double,
  val outFps: Double,
)

/**
 * Amendment 2 (E11): fps measured from two consecutive [FgStats] readings.
 *
 * @param nowMs injected monotonic clock in ms (by default `System.nanoTime() / 1e6`, no SystemClock).
 */
class FgFpsMeter(
  private val nowMs: () -> Long = { System.nanoTime() / 1_000_000L },
) {
  private var last: FgStats? = null
  private var lastMs = 0L

  /**
   * Takes the current reading (or null) and returns the fps between the previous reading and this
   * one. Returns null when this is the first reading, when the clock did not advance or went
   * backwards, or when a counter went down (pipeline restart). In those cases it re-seeds with the
   * current reading. A null reading clears the meter.
   */
  fun update(stats: FgStats?): FgFps? {
    val now = nowMs()
    val prev = last
    val prevMs = lastMs
    last = stats
    lastMs = now
    if (stats == null || prev == null) return null
    val dtMs = now - prevMs
    if (dtMs <= 0L) return null
    val dIn = stats.inFrames - prev.inFrames
    val dOut = stats.outFrames - prev.outFrames
    if (dIn < 0L || dOut < 0L) return null
    return FgFps(inFps = dIn * MS_PER_S / dtMs, outFps = dOut * MS_PER_S / dtMs)
  }

  /** Forgets the previous reading; the next [update] returns null. */
  fun reset() {
    last = null
    lastMs = 0L
  }

  private companion object {
    const val MS_PER_S = 1000.0
  }
}

/**
 * Amendment 2 (E11): structured view of an [FgUiState] (plus the measured [FgFps]) with the numbers
 * already rounded (fps and latency to 0.1, compute time to 0.01, audio compensation to 0.1 ms).
 *
 * @property reason the reason for [FgStatsStatus.FAILED] or [FgStatsStatus.SUSPENDED]; null otherwise.
 * @property noInputTimeout the failure came from the «no input frames» watchdog (only with FAILED).
 * @property nativeError `FgResult.fromCode(stats.lastError)` when `lastError != 0`, else null.
 * @property inFps measured (fps, only with [FgStatsStatus.ACTIVE]) or nominal `1e9 / periodNs`;
 *   0.0 when unknown.
 * @property outFps measured (fps, only with [FgStatsStatus.ACTIVE]) or nominal
 *   `inFps × multiplier`; 0.0 when unknown.
 * @property fpsMeasured the fps come from an [FgFps] measurement (not the nominal values); only
 *   possible with [FgStatsStatus.ACTIVE] (Amendment 3, E24).
 * @property hasStats `state.stats != null`; without stats every counter is 0.
 * @property overloaded Amendment 3 (E19): `state.overloaded` and the status is ACTIVE.
 * @property overloadInFps Amendment 3 (E19): `state.measuredInFps` rounded to 0.1 fps.
 * @property sourceFps Amendment 3 (E19): `state.sourceFps` rounded to 0.1 fps.
 */
data class FgStatsView(
  val status: FgStatsStatus,
  val reason: FgUnavailableReason?,
  val noInputTimeout: Boolean,
  val nativeError: FgResult?,
  val inFps: Double,
  val outFps: Double,
  val fpsMeasured: Boolean,
  val multiplier: Int,
  val refreshHz: Float,
  val inFrames: Long,
  val outFrames: Long,
  val generated: Long,
  val droppedLate: Long,
  val latencyMs: Double,
  val computeMs: Double,
  val presentPath: PresentPath,
  val inWidth: Int,
  val inHeight: Int,
  val audioCompMs: Double,
  val hasStats: Boolean,
  val overloaded: Boolean,
  val overloadInFps: Double,
  val sourceFps: Double,
) {
  /**
   * One fixed English line for logcat, e.g. `FG stats: status=ACTIVE in=29.9fps out=59.8fps N=2
   * hz=60.0 inF=120 outF=240 gen=120 late=0 lat=16.7ms comp=4.20ms path=DISPLAY_TIMING
   * in=1920x1080 audio=24ms err=-`. With a [reason] it adds `reason=<REASON>` (and `noInput` for the
   * watchdog) right after the status. When [overloaded] it ends with
   * `overloaded=<overloadInFps>/<sourceFps>fps` (Amendment 3, E19).
   */
  fun logLine(): String =
    buildString {
      append("FG stats: status=").append(status.name)
      if (reason != null) append(" reason=").append(reason.name)
      if (noInputTimeout) append(" noInput")
      append(String.format(Locale.US, " in=%.1ffps out=%.1ffps", inFps, outFps))
      append(" N=").append(multiplier)
      append(String.format(Locale.US, " hz=%.1f", refreshHz))
      append(" inF=").append(inFrames)
      append(" outF=").append(outFrames)
      append(" gen=").append(generated)
      append(" late=").append(droppedLate)
      append(String.format(Locale.US, " lat=%.1fms comp=%.2fms", latencyMs, computeMs))
      append(" path=").append(presentPath.name)
      append(" in=").append(inWidth).append('x').append(inHeight)
      append(String.format(Locale.US, " audio=%.0fms", audioCompMs))
      append(" err=").append(nativeError?.name ?: "-")
      if (overloaded) append(String.format(Locale.US, " overloaded=%.1f/%.1ffps", overloadInFps, sourceFps))
    }

  companion object {
    private const val NS_PER_S = 1_000_000_000.0
    private const val NS_PER_MS = 1_000_000.0

    /**
     * State rules, in order:
     * 1. FAILED if `state.unavailable?.kind == FAILED`.
     * 2. SUSPENDED if `state.unavailable?.kind == SUSPENDED`.
     * 3. PAUSED if `state.active` and `stats?.state == FgState.PAUSED`.
     * 4. ACTIVE if `state.active`.
     * 5. OFF otherwise (INELIGIBLE included).
     *
     * The counters come from `state.stats`; without stats they are 0 and `hasStats = false`.
     * The measured [fps] only count with status ACTIVE (Amendment 3, E24); without [fps], or with any
     * other status, the nominal rates are used (`1e9 / periodNs`, then × multiplier) and
     * `fpsMeasured = false`. [FgStatsView.overloaded] also requires ACTIVE.
     */
    fun from(
      state: FgUiState,
      fps: FgFps?,
    ): FgStatsView {
      val kind = state.unavailable?.kind
      val status =
        when {
          kind == FgUnavailableReason.Kind.FAILED -> FgStatsStatus.FAILED
          kind == FgUnavailableReason.Kind.SUSPENDED -> FgStatsStatus.SUSPENDED
          state.active && state.stats?.state == FgState.PAUSED -> FgStatsStatus.PAUSED
          state.active -> FgStatsStatus.ACTIVE
          else -> FgStatsStatus.OFF
        }
      val reason =
        if (status == FgStatsStatus.FAILED || status == FgStatsStatus.SUSPENDED) state.unavailable else null
      val s = state.stats
      val nominalIn = if (s != null && s.periodNs > 0L) NS_PER_S / s.periodNs else 0.0
      val nominalOut = if (nominalIn > 0.0 && state.multiplier > 0) nominalIn * state.multiplier else 0.0
      val measured = fps?.takeIf { status == FgStatsStatus.ACTIVE }
      return FgStatsView(
        status = status,
        reason = reason,
        noInputTimeout = state.noInputTimeout && status == FgStatsStatus.FAILED,
        nativeError = s?.lastError?.takeIf { it != 0 }?.let { FgResult.fromCode(it) },
        inFps = round1(measured?.inFps ?: nominalIn),
        outFps = round1(measured?.outFps ?: nominalOut),
        fpsMeasured = measured != null,
        multiplier = state.multiplier,
        refreshHz = state.refreshHz,
        inFrames = s?.inFrames ?: 0L,
        outFrames = s?.outFrames ?: 0L,
        generated = s?.generated ?: 0L,
        droppedLate = s?.droppedLate ?: 0L,
        latencyMs = round1((s?.latencyNs ?: 0L) / NS_PER_MS),
        computeMs = round2((s?.computeNsAvg ?: 0L) / NS_PER_MS),
        presentPath = s?.presentPath ?: PresentPath.NONE,
        inWidth = s?.inWidth ?: 0,
        inHeight = s?.inHeight ?: 0,
        audioCompMs = round1(state.audioCompMs),
        hasStats = s != null,
        overloaded = state.overloaded && status == FgStatsStatus.ACTIVE,
        overloadInFps = round1(state.measuredInFps),
        sourceFps = round1(state.sourceFps),
      )
    }

    private fun round1(v: Double): Double = if (v.isFinite()) round(v * 10.0) / 10.0 else 0.0

    private fun round2(v: Double): Double = if (v.isFinite()) round(v * 100.0) / 100.0 else 0.0
  }
}
