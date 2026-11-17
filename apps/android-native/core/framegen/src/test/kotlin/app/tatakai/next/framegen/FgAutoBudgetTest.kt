/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

package app.tatakai.next.framegen

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Amendment 3 (E18, D1b): [FgPolicy.autoBudgetStep]. */
class FgAutoBudgetTest {
  /** 23.976 fps source: ~41.7 ms period; 0.85 × 41.7 ms = 35.445 ms. */
  private val period = 41_700_000L

  private fun sample(
    nowMs: Long,
    auto: Boolean = true,
    n: Int = 4,
    counting: Boolean = true,
    outFrames: Long = 0L,
    droppedLate: Long = 0L,
    computeNsAvg: Long = 10_000_000L,
    sourcePeriodNs: Long = period,
  ) = FgBudgetSample(nowMs, auto, n, counting, outFrames, droppedLate, computeNsAvg, sourcePeriodNs)

  /** Feeds [samples] in order and returns every result. */
  private fun feed(start: FgBudgetState, samples: List<FgBudgetSample>): List<FgBudgetResult> {
    var state = start
    return samples.map { s -> FgPolicy.autoBudgetStep(state, s).also { state = it.state } }
  }

  /** Readings every 500 ms in 0..[untilMs] with 100 output frames per 500 ms. */
  private fun readings(
    untilMs: Long,
    n: Int = 4,
    computeNsAvg: Long = 10_000_000L,
    dropsPer100: Long = 0L,
    auto: Boolean = true,
    counting: Boolean = true,
  ) =
    (0L..untilMs step 500L).map { t ->
      val k = t / 500L
      sample(
        nowMs = t, auto = auto, n = n, counting = counting, outFrames = k * 100L, droppedLate = k * dropsPer100,
        computeNsAvg = computeNsAvg,
      )
    }

  @Test
  fun `no step with a fixed multiplier`() {
    val results = feed(FgBudgetState(startedMs = 0L), readings(20_000L, computeNsAvg = 40_000_000L, auto = false))
    for (r in results) {
      assertNull(r.stepTo)
      assertNull(r.state.windowStartMs)
    }
  }

  @Test
  fun `no step at the minimum multiplier`() {
    val results = feed(FgBudgetState(startedMs = 0L), readings(20_000L, n = 2, computeNsAvg = 40_000_000L, dropsPer100 = 50L))
    for (r in results) {
      assertNull(r.stepTo)
      assertNull(r.state.windowStartMs)
    }
  }

  @Test
  fun `no step while not counting`() {
    val results =
      feed(FgBudgetState(startedMs = 0L), readings(20_000L, computeNsAvg = 40_000_000L, counting = false))
    for (r in results) {
      assertNull(r.stepTo)
      assertNull(r.state.windowStartMs)
    }
  }

  @Test
  fun `no step before 3 s of running`() {
    val start = FgBudgetState(startedMs = 0L)
    // t=0 opens the window; t=2000 closes it over budget but only 2000 ms ran → new window.
    val r0 = FgPolicy.autoBudgetStep(start, sample(0L, computeNsAvg = 40_000_000L))
    assertNull(r0.stepTo)
    assertEquals(0L, r0.state.windowStartMs)
    val r1 = FgPolicy.autoBudgetStep(r0.state, sample(2_000L, outFrames = 400L, computeNsAvg = 40_000_000L))
    assertNull(r1.stepTo)
    assertEquals(2_000L, r1.state.windowStartMs)
    assertEquals(400L, r1.state.windowOutFrames)
    // t=4000: window of 2 s, 4000 ms running → step.
    val r2 = FgPolicy.autoBudgetStep(r1.state, sample(4_000L, outFrames = 800L, computeNsAvg = 40_000_000L))
    assertEquals(3, r2.stepTo)
    assertEquals(4_000L, r2.state.lastStepMs)
    assertNull(r2.state.windowStartMs)
  }

  @Test
  fun `the window must last 2 s`() {
    val start = FgBudgetState(startedMs = -10_000L)
    val r0 = FgPolicy.autoBudgetStep(start, sample(0L, computeNsAvg = 40_000_000L))
    val r1 = FgPolicy.autoBudgetStep(r0.state, sample(1_999L, outFrames = 400L, computeNsAvg = 40_000_000L))
    assertNull(r1.stepTo)
    assertEquals(r0.state, r1.state)
    val r2 = FgPolicy.autoBudgetStep(r1.state, sample(2_000L, outFrames = 400L, computeNsAvg = 40_000_000L))
    assertEquals(3, r2.stepTo)
  }

  @Test
  fun `compute over 85 percent of the source period steps down`() {
    // 0.85 × 41.7 ms = 35.445 ms: 36 ms is over, 35 ms is not.
    val over = feed(FgBudgetState(startedMs = 0L), readings(4_000L, computeNsAvg = 36_000_000L))
    assertEquals(listOf<Int?>(null, null, null, null, null, null, null, null, 3), over.map { it.stepTo })
    val under = feed(FgBudgetState(startedMs = 0L), readings(20_000L, computeNsAvg = 35_000_000L))
    for (r in under) assertNull(r.stepTo)
    // An unknown source period never counts as compute over budget.
    val unknown = feed(
      FgBudgetState(startedMs = 0L),
      (0L..20_000L step 500L).map { t -> sample(t, outFrames = t / 5, computeNsAvg = 90_000_000L, sourcePeriodNs = 0L) },
    )
    for (r in unknown) assertNull(r.stepTo)
  }

  @Test
  fun `drops over 10 percent of the output step down`() {
    val start = FgBudgetState(startedMs = 0L)
    val base = FgPolicy.autoBudgetStep(start, sample(2_000L, outFrames = 1_000L, droppedLate = 7L)).state
    // 11 dropped of 100 out → over.
    val over = FgPolicy.autoBudgetStep(base, sample(4_000L, outFrames = 1_100L, droppedLate = 18L))
    assertEquals(3, over.stepTo)
    // 10 dropped of 100 out → not over (strictly greater).
    val under = FgPolicy.autoBudgetStep(base, sample(4_000L, outFrames = 1_100L, droppedLate = 17L))
    assertNull(under.stepTo)
    assertEquals(4_000L, under.state.windowStartMs)
    assertEquals(1_100L, under.state.windowOutFrames)
    assertEquals(17L, under.state.windowDropped)
    // No output but some drops → over.
    val stalled = FgPolicy.autoBudgetStep(base, sample(4_000L, outFrames = 1_000L, droppedLate = 8L))
    assertEquals(3, stalled.stepTo)
    // No output and no drops → not over.
    val idle = FgPolicy.autoBudgetStep(base, sample(4_000L, outFrames = 1_000L, droppedLate = 7L))
    assertNull(idle.stepTo)
  }

  @Test
  fun `at least 5 s between steps`() {
    // Always over budget, readings every 500 ms; the caller applies each step (n goes down).
    var state = FgBudgetState(startedMs = 0L)
    var n = 5
    val steps = mutableListOf<Pair<Long, Int>>()
    for (t in 0L..20_000L step 500L) {
      val r = FgPolicy.autoBudgetStep(state, sample(t, n = n, outFrames = t / 5, computeNsAvg = 40_000_000L))
      state = r.state
      r.stepTo?.let {
        steps += t to it
        n = it
      }
    }
    // 4000: first step (3 s running, 2 s window). 4500 opens a window; 6500 is 2.5 s after the step
    // and 8500 is 4.5 s after → new windows; 10500: 6.5 s after → step. Same again: 11000 opens,
    // 13000 and 15000 are too soon, 17000 steps to 2. At the minimum the rule stops.
    assertEquals(listOf(4_000L to 4, 10_500L to 3, 17_000L to 2), steps)
    for (i in 1 until steps.size) {
      assertTrue(steps[i].first - steps[i - 1].first >= FgPolicy.BUDGET_MIN_STEP_INTERVAL_MS)
    }
  }

  @Test
  fun `a counter reset reopens the window`() {
    val start = FgBudgetState(startedMs = 0L)
    val r0 = FgPolicy.autoBudgetStep(start, sample(0L, outFrames = 1_000L, droppedLate = 50L))
    assertEquals(0L, r0.state.windowStartMs)
    // Output counter went down (pipeline restarted) → new window at 2000, no step even if 2 s passed.
    val r1 = FgPolicy.autoBudgetStep(r0.state, sample(2_000L, outFrames = 100L, droppedLate = 0L))
    assertNull(r1.stepTo)
    assertEquals(FgBudgetState(startedMs = 0L, windowStartMs = 2_000L, windowOutFrames = 100L, windowDropped = 0L), r1.state)
    // Dropped counter went down alone → new window too.
    val r1b = FgPolicy.autoBudgetStep(r0.state, sample(2_000L, outFrames = 1_500L, droppedLate = 10L))
    assertNull(r1b.stepTo)
    assertEquals(2_000L, r1b.state.windowStartMs)
    // From the new window: 20 dropped of 100 → step at 4000.
    val r2 = FgPolicy.autoBudgetStep(r1.state, sample(4_000L, outFrames = 200L, droppedLate = 20L))
    assertEquals(3, r2.stepTo)
  }

  @Test
  fun `constants match the design`() {
    assertEquals(3_000L, FgPolicy.BUDGET_MIN_RUNNING_MS)
    assertEquals(5_000L, FgPolicy.BUDGET_MIN_STEP_INTERVAL_MS)
    assertEquals(2_000L, FgPolicy.BUDGET_WINDOW_MS)
    assertEquals(0.85, FgPolicy.BUDGET_COMPUTE_FRACTION, 0.0)
    assertEquals(0.10, FgPolicy.BUDGET_DROP_FRACTION, 0.0)
    assertEquals(3, FgPolicy.AUTO_EXPERIMENTAL_MAX_MULTIPLIER)
  }
}
