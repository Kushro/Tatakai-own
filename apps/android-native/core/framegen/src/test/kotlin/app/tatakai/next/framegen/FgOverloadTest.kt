/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

package app.tatakai.next.framegen

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Test

/** Amendment 3 (E19, D1c): [FgPolicy.overloadStep]. */
class FgOverloadTest {
  /** Feeds a reading of [inFps] every 500 ms in 0..[untilMs] and returns the final state. */
  private fun feed(
    untilMs: Long,
    inFps: Double,
    sourceFps: Double = 30.0,
    start: FgOverloadState = FgOverloadState(),
    fromMs: Long = 0L,
  ): FgOverloadState {
    var state = start
    for (t in fromMs..untilMs step 500L) state = FgPolicy.overloadStep(state, t, inFps, sourceFps, counting = true)
    return state
  }

  @Test
  fun `20 of 30 fps for more than 5 s is an overload`() {
    // 20 < 0.8 × 30 = 24. Below since 0; overloaded when now − 0 > 5000.
    val at4500 = feed(4_500L, 20.0)
    assertEquals(0L, at4500.belowSinceMs)
    assertFalse(at4500.overloaded)
    val at5000 = feed(5_000L, 20.0)
    assertFalse(at5000.overloaded) // strictly more than 5 s
    val at5500 = feed(5_500L, 20.0)
    assertTrue(at5500.overloaded)
    assertEquals(FgOverloadState(belowSinceMs = 0L, overloaded = true, inFps = 20.0), at5500)
  }

  @Test
  fun `a reading at or above 80 percent restarts`() {
    val overloaded = feed(6_000L, 20.0)
    assertTrue(overloaded.overloaded)
    // (20 + 30) / 2 = 25 ≥ 24 → restart.
    val restarted = FgPolicy.overloadStep(overloaded, 6_500L, 30.0, 30.0, counting = true)
    assertEquals(FgOverloadState(inFps = 25.0), restarted)
    // Next low reading: (25 + 20) / 2 = 22.5 < 24 → below again since 7000, not overloaded.
    val again = FgPolicy.overloadStep(restarted, 7_000L, 20.0, 30.0, counting = true)
    assertEquals(FgOverloadState(belowSinceMs = 7_000L, overloaded = false, inFps = 22.5), again)
    // It needs more than 5 s from 7000 again.
    assertFalse(feed(12_000L, 20.0, start = again, fromMs = 7_500L).overloaded)
    assertTrue(feed(12_500L, 20.0, start = again, fromMs = 7_500L).overloaded)
  }

  @Test
  fun `exactly 80 percent is not below the threshold`() {
    val s = FgPolicy.overloadStep(FgOverloadState(), 0L, 24.0, 30.0, counting = true)
    assertNull(s.belowSinceMs)
    assertEquals(24.0, s.inFps, 0.0)
  }

  @Test
  fun `not counting restarts`() {
    val overloaded = feed(6_000L, 20.0)
    assertTrue(overloaded.overloaded)
    assertEquals(FgOverloadState(), FgPolicy.overloadStep(overloaded, 6_500L, 20.0, 30.0, counting = false))
    assertEquals(FgOverloadState(), FgPolicy.overloadStep(overloaded, 6_500L, null, null, counting = false))
  }

  @Test
  fun `unknown readings leave the state unchanged`() {
    val state = feed(3_000L, 20.0)
    assertSame(state, FgPolicy.overloadStep(state, 3_500L, null, 30.0, counting = true))
    assertSame(state, FgPolicy.overloadStep(state, 3_500L, Double.NaN, 30.0, counting = true))
    assertSame(state, FgPolicy.overloadStep(state, 3_500L, Double.POSITIVE_INFINITY, 30.0, counting = true))
    assertSame(state, FgPolicy.overloadStep(state, 3_500L, 20.0, null, counting = true))
    assertSame(state, FgPolicy.overloadStep(state, 3_500L, 20.0, 0.0, counting = true))
    assertSame(state, FgPolicy.overloadStep(state, 3_500L, 20.0, -30.0, counting = true))
    // An unknown source never starts the timer either.
    assertEquals(FgOverloadState(), feed(20_000L, 5.0, sourceFps = 0.0))
  }

  @Test
  fun `smoothing averages two readings`() {
    // First reading: no previous value → taken as is.
    assertEquals(20.0, FgPolicy.overloadStep(FgOverloadState(), 0L, 20.0, 30.0, true).inFps, 0.0)
    // Then the mean of the previous smoothed value and the new reading.
    assertEquals(25.0, FgPolicy.overloadStep(FgOverloadState(inFps = 20.0), 0L, 30.0, 30.0, true).inFps, 0.0)
    assertEquals(22.5, FgPolicy.overloadStep(FgOverloadState(inFps = 25.0), 0L, 20.0, 30.0, true).inFps, 0.0)
    // Deterministic: the same input gives the same output.
    val a = FgPolicy.overloadStep(FgOverloadState(belowSinceMs = 100L, inFps = 21.0), 900L, 19.0, 30.0, true)
    val b = FgPolicy.overloadStep(FgOverloadState(belowSinceMs = 100L, inFps = 21.0), 900L, 19.0, 30.0, true)
    assertEquals(a, b)
    assertEquals(FgOverloadState(belowSinceMs = 100L, overloaded = false, inFps = 20.0), a)
    // A single short dip is absorbed: 30 then one reading of 18 → (30 + 18) / 2 = 24, not below.
    val dip = FgPolicy.overloadStep(FgOverloadState(inFps = 30.0), 0L, 18.0, 30.0, true)
    assertNull(dip.belowSinceMs)
  }

  @Test
  fun `constants match the design`() {
    assertEquals(0.80, FgPolicy.OVERLOAD_INPUT_FRACTION, 0.0)
    assertEquals(5_000L, FgPolicy.OVERLOAD_MIN_MS)
  }
}
