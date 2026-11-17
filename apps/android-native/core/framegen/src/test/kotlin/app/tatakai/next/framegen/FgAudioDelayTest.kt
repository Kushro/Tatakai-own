/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

package app.tatakai.next.framegen

import org.junit.Assert.assertEquals
import org.junit.Test

class FgAudioDelayTest {
  private val eps = 1e-9
  private val userValues = listOf(0.0, 0.25, -0.3, 1.5, -2.0)
  private val latencies = listOf(0.0, 16.7, 33.3, 48.0, 120.5)
  private val offsets = listOf(-500, -125, -5, 0, 5, 90, 500)

  @Test
  fun `compose adds latency and offset in seconds`() {
    assertEquals(0.25 + 0.040 + 0.010, FgAudioDelay.compose(0.25, 40.0, 10), eps)
    assertEquals(-0.1 + 0.030 - 0.500, FgAudioDelay.compose(-0.1, 30.0, -500), eps)
    assertEquals(0.0, FgAudioDelay.compensationSec(0.0, 0), eps)
    assertEquals(0.020 - 0.125, FgAudioDelay.compensationSec(20.0, -125), eps)
  }

  @Test
  fun `round trip compose then userFrom returns the user value`() {
    for (user in userValues) for (latency in latencies) for (offset in offsets) {
      val effective = FgAudioDelay.compose(user, latency, offset)
      val back = FgAudioDelay.userFrom(effective, FgAudioDelay.compensationSec(latency, offset))
      assertEquals("user=$user L=$latency offset=$offset", user, back, eps)
    }
  }

  @Test
  fun `latency never appears in the user value`() {
    // I6: for a fixed user value and offset, the persisted/shown value is the same whatever L is.
    for (user in userValues) for (offset in offsets) {
      val shown =
        latencies.map { l ->
          FgAudioDelay.userFrom(FgAudioDelay.compose(user, l, offset), FgAudioDelay.compensationSec(l, offset))
        }
      for (value in shown) assertEquals("user=$user offset=$offset", user, value, eps)
    }
  }

  @Test
  fun `user changes made on the effective value keep only the user part`() {
    // The user nudges the delay by +0.1 s while FG is active with L = 48 ms and offset -20 ms.
    val comp = FgAudioDelay.compensationSec(48.0, -20)
    val effective = FgAudioDelay.compose(0.2, 48.0, -20) + 0.1
    assertEquals(0.3, FgAudioDelay.userFrom(effective, comp), eps)
  }
}
