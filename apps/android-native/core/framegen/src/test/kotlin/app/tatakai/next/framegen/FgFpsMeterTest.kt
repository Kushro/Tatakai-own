/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

package app.tatakai.next.framegen

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Amendment 2 (E11): fps from two readings with an injected clock. */
class FgFpsMeterTest {
  private val eps = 1e-9

  private var clockMs = 0L
  private val meter = FgFpsMeter { clockMs }

  private fun stats(
    inFrames: Long,
    outFrames: Long,
  ): FgStats = FgStats.EMPTY.copy(inFrames = inFrames, outFrames = outFrames, state = FgState.RUNNING)

  @Test
  fun `the first reading has no fps`() {
    clockMs = 1_000L
    assertNull(meter.update(stats(10, 20)))
  }

  @Test
  fun `two readings give the input and output rates`() {
    clockMs = 1_000L
    assertNull(meter.update(stats(100, 200)))
    clockMs = 1_500L
    val fps = checkNotNull(meter.update(stats(115, 230)))
    assertEquals(30.0, fps.inFps, eps)
    assertEquals(60.0, fps.outFps, eps)
  }

  @Test
  fun `each reading is measured against the previous one`() {
    clockMs = 0L
    meter.update(stats(0, 0))
    clockMs = 1_000L
    meter.update(stats(24, 48))
    clockMs = 2_000L
    val fps = checkNotNull(meter.update(stats(54, 108)))
    assertEquals(30.0, fps.inFps, eps)
    assertEquals(60.0, fps.outFps, eps)
  }

  @Test
  fun `a clock that goes backwards or stays gives null and re-seeds`() {
    clockMs = 2_000L
    meter.update(stats(100, 200))
    clockMs = 1_500L
    assertNull(meter.update(stats(110, 220)))
    clockMs = 1_500L
    assertNull(meter.update(stats(120, 240)))
    // Re-seeded with the last reading (t=1500, 120/240).
    clockMs = 2_500L
    val fps = checkNotNull(meter.update(stats(150, 300)))
    assertEquals(30.0, fps.inFps, eps)
    assertEquals(60.0, fps.outFps, eps)
  }

  @Test
  fun `counters that go down give null and re-seed`() {
    clockMs = 0L
    meter.update(stats(500, 1000))
    clockMs = 500L
    assertNull(meter.update(stats(3, 6)))
    clockMs = 1_000L
    assertNull(meter.update(stats(20, 5)))
    clockMs = 2_000L
    val fps = checkNotNull(meter.update(stats(50, 65)))
    assertEquals(30.0, fps.inFps, eps)
    assertEquals(60.0, fps.outFps, eps)
  }

  @Test
  fun `a null reading or reset clears the meter`() {
    clockMs = 0L
    meter.update(stats(0, 0))
    clockMs = 500L
    assertNull(meter.update(null))
    clockMs = 1_000L
    assertNull(meter.update(stats(30, 60)))

    meter.reset()
    clockMs = 2_000L
    assertNull(meter.update(stats(60, 120)))
  }
}
