/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

/** R13: presentation path of the native presenter (FgStats index 7, `efg::PresentPath`). */
enum class PresentPath(val code: Int) {
  NONE(0),
  DISPLAY_TIMING(1),
  SLEEP_VSYNC(2),
  CPU(3),
  ;

  companion object {
    /** Unknown codes map to [NONE]. */
    fun fromCode(code: Long): PresentPath = entries.firstOrNull { it.code.toLong() == code } ?: NONE
  }
}

/** R13: pipeline state (FgStats index 8, `efg::FgStatus`). [FAILED] is terminal for the file (I9). */
enum class FgState(val code: Int) {
  STOPPED(0),
  RUNNING(1),
  PAUSED(2),
  FAILED(3),
  ;

  companion object {
    /** Unknown codes map to [STOPPED]. */
    fun fromCode(code: Long): FgState = entries.firstOrNull { it.code.toLong() == code } ?: STOPPED
  }
}

/**
 * R13: one snapshot of [FgNative.stats], decoded with the fixed indices of `efg::FgStatIndex`.
 *
 * While running these are the live engine counters (index 7 from the presenter). After
 * [FgNative.stop] they are the last snapshot taken during stop, with RUNNING/PAUSED reported as
 * STOPPED (FAILED is kept). Never started: all zeros ([EMPTY]).
 *
 * @property generated Amendment 3 (E21): generated frames that were actually **presented** (counted
 *   when the presenter returns the slot, not when it is published), so it is a subset of
 *   [outFrames] (`generated ≤ outFrames`).
 * @property lastError an [FgResult] code (0 = none); resolve it with [FgResult.fromCode].
 */
data class FgStats(
  val inFrames: Long,
  val outFrames: Long,
  val generated: Long,
  val droppedLate: Long,
  val latencyNs: Long,
  val computeNsAvg: Long,
  val periodNs: Long,
  val presentPath: PresentPath,
  val state: FgState,
  val lastError: Int,
  val inWidth: Int,
  val inHeight: Int,
) {
  companion object {
    const val IDX_IN_FRAMES = 0
    const val IDX_OUT_FRAMES = 1
    const val IDX_GENERATED = 2
    const val IDX_DROPPED_LATE = 3
    const val IDX_LATENCY_NS = 4
    const val IDX_COMPUTE_NS_AVG = 5
    const val IDX_PERIOD_NS = 6
    const val IDX_PRESENT_PATH = 7
    const val IDX_STATE = 8
    const val IDX_LAST_ERROR = 9
    const val IDX_IN_W = 10
    const val IDX_IN_H = 11

    /** Length of the native stats array (`kStatCount`). */
    const val SIZE = 12

    /** All zeros: never started. */
    val EMPTY: FgStats = from(LongArray(SIZE))

    /** Decodes [values]; missing trailing entries read as 0, extra ones are ignored. */
    fun from(values: LongArray): FgStats {
      fun at(i: Int): Long = values.getOrElse(i) { 0L }
      return FgStats(
        inFrames = at(IDX_IN_FRAMES),
        outFrames = at(IDX_OUT_FRAMES),
        generated = at(IDX_GENERATED),
        droppedLate = at(IDX_DROPPED_LATE),
        latencyNs = at(IDX_LATENCY_NS),
        computeNsAvg = at(IDX_COMPUTE_NS_AVG),
        periodNs = at(IDX_PERIOD_NS),
        presentPath = PresentPath.fromCode(at(IDX_PRESENT_PATH)),
        state = FgState.fromCode(at(IDX_STATE)),
        lastError = at(IDX_LAST_ERROR).toInt(),
        inWidth = at(IDX_IN_W).toInt(),
        inHeight = at(IDX_IN_H).toInt(),
      )
    }
  }
}
