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
 * R13: effective state of mpv's cadence smoothing (E39, `interpolation_enabled`) under FG.
 *
 * Fact 17 / I7: while FG is active E39 is forced off, and when FG turns off the user's value comes
 * back. E39 is disabled only through this rule; the user preference itself is never rewritten.
 */
object FgInterpolationRule {
  fun effective(
    userEnabled: Boolean,
    fgActive: Boolean,
  ): Boolean = userEnabled && !fgActive
}
