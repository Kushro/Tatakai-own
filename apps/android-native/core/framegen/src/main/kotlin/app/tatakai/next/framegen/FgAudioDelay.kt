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
 * R13: composition of mpv's `audio-delay` while FG is active (fact 8).
 *
 * Effective delay = user delay + pipeline latency L + manual FG offset. L is measured by the
 * pipeline and the offset is the global `FgPreferences.audioOffsetMs` (−500..+500 ms).
 *
 * I6 (fact 9): L and the offset never reach `PlaybackStateEntity.audioDelay` nor the audio-delay
 * panel. Only the result of [userFrom] is persisted or shown.
 */
object FgAudioDelay {
  /** Effective `audio-delay` in seconds for the user value [userSec]. */
  fun compose(
    userSec: Double,
    latencyMs: Double,
    offsetMs: Int,
  ): Double = userSec + compensationSec(latencyMs, offsetMs)

  /** FG part of the delay (L + manual offset) in seconds. */
  fun compensationSec(
    latencyMs: Double,
    offsetMs: Int,
  ): Double = (latencyMs + offsetMs) / 1000.0

  /** User part of an effective delay: the only value that may be persisted or shown (I6). */
  fun userFrom(
    effectiveSec: Double,
    compensationSec: Double,
  ): Double = effectiveSec - compensationSec
}
