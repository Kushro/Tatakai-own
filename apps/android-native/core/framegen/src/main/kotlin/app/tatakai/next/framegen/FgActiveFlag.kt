/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * R13: whether the FG pipeline is presenting right now.
 *
 * Written only by `FgController`; read by the quality/mpv option code (through
 * [FgInterpolationRule]) so that neither side depends on the other, which breaks the cycle
 * FgController <-> QualityController/MpvOptionsApplier.
 */
class FgActiveFlag {
  private val _active = MutableStateFlow(false)
  val active: StateFlow<Boolean> = _active.asStateFlow()

  fun set(value: Boolean) {
    _active.value = value
  }
}
