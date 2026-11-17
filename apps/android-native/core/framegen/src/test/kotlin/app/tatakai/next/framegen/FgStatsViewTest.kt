/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

package app.tatakai.next.framegen

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Amendment 2 (E11): status, reason and numbers of the FG stats overlay and logcat line. */
class FgStatsViewTest {
  private val eps = 1e-9

  private val running =
    FgStats(
      inFrames = 120,
      outFrames = 240,
      generated = 120,
      droppedLate = 1,
      latencyNs = 16_700_000,
      computeNsAvg = 4_200_000,
      periodNs = 41_708_333, // 23.976 fps
      presentPath = PresentPath.DISPLAY_TIMING,
      state = FgState.RUNNING,
      lastError = 0,
      inWidth = 1920,
      inHeight = 1080,
    )

  private val activeState =
    FgUiState(
      enabled = true,
      active = true,
      multiplier = 2,
      auto = false,
      stats = running,
      audioCompMs = 24.0,
      refreshHz = 60f,
    )

  @Test
  fun `active with stats is ACTIVE with no reason`() {
    val v = FgStatsView.from(activeState, FgFps(29.94, 59.88))
    assertEquals(FgStatsStatus.ACTIVE, v.status)
    assertNull(v.reason)
    assertFalse(v.noInputTimeout)
    assertNull(v.nativeError)
    assertTrue(v.hasStats)
    assertTrue(v.fpsMeasured)
    assertEquals(29.9, v.inFps, eps)
    assertEquals(59.9, v.outFps, eps)
    assertEquals(2, v.multiplier)
    assertEquals(60f, v.refreshHz, 0f)
    assertEquals(120L, v.inFrames)
    assertEquals(240L, v.outFrames)
    assertEquals(120L, v.generated)
    assertEquals(1L, v.droppedLate)
    assertEquals(16.7, v.latencyMs, eps)
    assertEquals(4.2, v.computeMs, eps)
    assertEquals(PresentPath.DISPLAY_TIMING, v.presentPath)
    assertEquals(1920, v.inWidth)
    assertEquals(1080, v.inHeight)
    assertEquals(24.0, v.audioCompMs, eps)
  }

  @Test
  fun `active with paused stats is PAUSED`() {
    val state = activeState.copy(stats = running.copy(state = FgState.PAUSED))
    assertEquals(FgStatsStatus.PAUSED, FgStatsView.from(state, null).status)
  }

  @Test
  fun `a single reading falls back to the nominal rates`() {
    val v = FgStatsView.from(activeState, null)
    assertFalse(v.fpsMeasured)
    assertEquals(24.0, v.inFps, eps) // 1e9 / 41_708_333 = 23.976 -> 24.0
    assertEquals(48.0, v.outFps, eps) // 23.976 x 2 = 47.95 -> 48.0
  }

  @Test
  fun `unknown period gives zero nominal rates`() {
    val v = FgStatsView.from(activeState.copy(stats = running.copy(periodNs = 0)), null)
    assertEquals(0.0, v.inFps, eps)
    assertEquals(0.0, v.outFps, eps)
  }

  @Test
  fun `watchdog failure is FAILED with START_FAILED and noInputTimeout, keeping the stats`() {
    val state =
      FgUiState(
        enabled = true,
        active = false,
        multiplier = 2,
        unavailable = FgUnavailableReason.START_FAILED,
        noInputTimeout = true,
        stats = running.copy(inFrames = 0, outFrames = 0, generated = 0, state = FgState.STOPPED),
      )
    val v = FgStatsView.from(state, null)
    assertEquals(FgStatsStatus.FAILED, v.status)
    assertEquals(FgUnavailableReason.START_FAILED, v.reason)
    assertTrue(v.noInputTimeout)
    assertTrue(v.hasStats)
    assertEquals(0L, v.inFrames)
    assertEquals(PresentPath.DISPLAY_TIMING, v.presentPath)
  }

  @Test
  fun `device lost is FAILED with DEVICE_LOST and the native error`() {
    val state =
      FgUiState(
        active = false,
        unavailable = FgUnavailableReason.DEVICE_LOST,
        stats = running.copy(state = FgState.FAILED, lastError = FgResult.DEVICE_LOST.code),
      )
    val v = FgStatsView.from(state, null)
    assertEquals(FgStatsStatus.FAILED, v.status)
    assertEquals(FgUnavailableReason.DEVICE_LOST, v.reason)
    assertFalse(v.noInputTimeout)
    assertEquals(FgResult.DEVICE_LOST, v.nativeError)
    assertTrue(v.hasStats)
  }

  @Test
  fun `a failure keeps its stats while FG is not active`() {
    val state = FgUiState(active = false, unavailable = FgUnavailableReason.START_FAILED, stats = running)
    val v = FgStatsView.from(state, null)
    assertEquals(FgStatsStatus.FAILED, v.status)
    assertTrue(v.hasStats)
    assertEquals(120L, v.inFrames)
    assertEquals(240L, v.outFrames)
  }

  @Test
  fun `suspension is SUSPENDED with its reason and no watchdog mark`() {
    val state = FgUiState(enabled = true, unavailable = FgUnavailableReason.PIP, noInputTimeout = true)
    val v = FgStatsView.from(state, null)
    assertEquals(FgStatsStatus.SUSPENDED, v.status)
    assertEquals(FgUnavailableReason.PIP, v.reason)
    assertFalse(v.noInputTimeout)
  }

  @Test
  fun `off without stats is OFF with zero counters`() {
    val v = FgStatsView.from(FgUiState(), null)
    assertEquals(FgStatsStatus.OFF, v.status)
    assertNull(v.reason)
    assertNull(v.nativeError)
    assertFalse(v.hasStats)
    assertFalse(v.fpsMeasured)
    assertEquals(0L, v.inFrames)
    assertEquals(0L, v.outFrames)
    assertEquals(0.0, v.inFps, eps)
    assertEquals(0.0, v.outFps, eps)
    assertEquals(PresentPath.NONE, v.presentPath)
  }

  @Test
  fun `ineligible is OFF with no reason`() {
    val v = FgStatsView.from(FgUiState(unavailable = FgUnavailableReason.VULKAN), null)
    assertEquals(FgStatsStatus.OFF, v.status)
    assertNull(v.reason)
  }

  @Test
  fun `log line carries status, rates and counters`() {
    val line = FgStatsView.from(activeState, FgFps(29.94, 59.88)).logLine()
    assertTrue(line, line.startsWith("FG stats: "))
    assertTrue(line, line.contains("status=ACTIVE"))
    assertTrue(line, line.contains("in=29.9fps"))
    assertTrue(line, line.contains("out=59.9fps"))
    assertTrue(line, line.contains("N=2"))
    assertTrue(line, line.contains("hz=60.0"))
    assertTrue(line, line.contains("inF=120"))
    assertTrue(line, line.contains("lat=16.7ms"))
    assertTrue(line, line.contains("comp=4.20ms"))
    assertTrue(line, line.contains("path=DISPLAY_TIMING"))
    assertTrue(line, line.contains("in=1920x1080"))
    assertTrue(line, line.contains("audio=24ms"))
    assertTrue(line, line.contains("err=-"))
  }

  @Test
  fun `log line of a failure carries the reason and the native error`() {
    val state =
      FgUiState(
        unavailable = FgUnavailableReason.DEVICE_LOST,
        stats = running.copy(state = FgState.FAILED, lastError = FgResult.DEVICE_LOST.code),
      )
    val line = FgStatsView.from(state, null).logLine()
    assertTrue(line, line.contains("status=FAILED"))
    assertTrue(line, line.contains("reason=DEVICE_LOST"))
    assertTrue(line, line.contains("err=DEVICE_LOST"))
  }

  // Amendment 3 (E24): measured fps only count while ACTIVE.

  @Test
  fun `paused with a measurement shows the nominal rates`() {
    val state = activeState.copy(stats = running.copy(state = FgState.PAUSED))
    val v = FgStatsView.from(state, FgFps(29.94, 59.88))
    assertEquals(FgStatsStatus.PAUSED, v.status)
    assertFalse(v.fpsMeasured)
    assertEquals(24.0, v.inFps, eps) // nominal 1e9 / 41_708_333 = 23.976 -> 24.0
    assertEquals(48.0, v.outFps, eps) // 23.976 x 2 = 47.95 -> 48.0
  }

  @Test
  fun `failed with a measurement shows the nominal rates`() {
    val state =
      FgUiState(active = false, multiplier = 2, unavailable = FgUnavailableReason.START_FAILED, stats = running)
    val v = FgStatsView.from(state, FgFps(29.94, 59.88))
    assertEquals(FgStatsStatus.FAILED, v.status)
    assertFalse(v.fpsMeasured)
    assertEquals(24.0, v.inFps, eps)
    assertEquals(48.0, v.outFps, eps)
  }

  @Test
  fun `active with a measurement shows the measured rates`() {
    val v = FgStatsView.from(activeState, FgFps(12.34, 24.66))
    assertEquals(FgStatsStatus.ACTIVE, v.status)
    assertTrue(v.fpsMeasured)
    assertEquals(12.3, v.inFps, eps)
    assertEquals(24.7, v.outFps, eps)
  }

  // Amendment 3 (E19): overload.

  @Test
  fun `overload is reported while ACTIVE with rounded rates`() {
    val state = activeState.copy(overloaded = true, measuredInFps = 18.26, sourceFps = 23.976)
    val v = FgStatsView.from(state, FgFps(18.26, 36.52))
    assertTrue(v.overloaded)
    assertEquals(18.3, v.overloadInFps, eps)
    assertEquals(24.0, v.sourceFps, eps)
  }

  @Test
  fun `overload is not reported when FAILED or PAUSED`() {
    val failed =
      FgUiState(
        active = false,
        unavailable = FgUnavailableReason.DEVICE_LOST,
        stats = running,
        overloaded = true,
        measuredInFps = 18.26,
        sourceFps = 23.976,
      )
    assertFalse(FgStatsView.from(failed, FgFps(18.26, 36.52)).overloaded)
    val paused =
      activeState.copy(stats = running.copy(state = FgState.PAUSED), overloaded = true, sourceFps = 23.976)
    assertFalse(FgStatsView.from(paused, null).overloaded)
  }

  @Test
  fun `not overloaded by default`() {
    val v = FgStatsView.from(activeState, FgFps(29.94, 59.88))
    assertFalse(v.overloaded)
    assertEquals(0.0, v.overloadInFps, eps)
    assertEquals(0.0, v.sourceFps, eps)
  }

  @Test
  fun `log line carries the overload only when overloaded`() {
    val overloaded = activeState.copy(overloaded = true, measuredInFps = 18.26, sourceFps = 23.976)
    val line = FgStatsView.from(overloaded, FgFps(18.26, 36.52)).logLine()
    assertTrue(line, line.endsWith(" overloaded=18.3/24.0fps"))

    val normal = FgStatsView.from(activeState.copy(sourceFps = 23.976), FgFps(29.94, 59.88)).logLine()
    assertFalse(normal, normal.contains("overloaded="))

    val failed =
      FgUiState(
        unavailable = FgUnavailableReason.DEVICE_LOST,
        stats = running,
        overloaded = true,
        measuredInFps = 18.26,
        sourceFps = 23.976,
      )
    val failedLine = FgStatsView.from(failed, null).logLine()
    assertFalse(failedLine, failedLine.contains("overloaded="))
  }
}
