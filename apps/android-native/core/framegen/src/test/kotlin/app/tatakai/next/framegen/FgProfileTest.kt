/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

package app.tatakai.next.framegen

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FgProfileTest {
  @Test
  fun `presets match fact 25 exactly`() {
    assertEquals(FgTuning(performanceMode = true, flowScale = 0.5f, inputScale = 0.75f), FgProfile.SAVER.presetTuning())
    assertEquals(FgTuning(performanceMode = true, flowScale = 0.5f, inputScale = 1.0f), FgProfile.BALANCED.presetTuning())
    assertEquals(FgTuning(performanceMode = false, flowScale = 1.0f, inputScale = 1.0f), FgProfile.QUALITY.presetTuning())
  }

  @Test
  fun `custom has no preset`() {
    assertNull(FgProfile.CUSTOM.presetTuning())
  }

  @Test
  fun `profiles are exactly saver balanced quality custom`() {
    assertEquals(
      listOf(FgProfile.SAVER, FgProfile.BALANCED, FgProfile.QUALITY, FgProfile.CUSTOM),
      FgProfile.entries.toList(),
    )
  }

  @Test
  fun `every preset is inside the custom ranges`() {
    for (p in FgProfile.entries) {
      val t = p.presetTuning() ?: continue
      assertTrue("profile=$p", t.isValidCustom())
    }
  }

  @Test
  fun `custom ranges reject out-of-range values`() {
    assertTrue(FgTuning(true, 0.25f, 0.75f).isValidCustom())
    assertTrue(FgTuning(false, 1.0f, 1.0f).isValidCustom())
    assertFalse("flow below min", FgTuning(true, 0.2f, 1.0f).isValidCustom())
    assertFalse("flow above max", FgTuning(true, 1.1f, 1.0f).isValidCustom())
    assertFalse("input 50 %", FgTuning(true, 0.5f, 0.5f).isValidCustom())
    assertFalse("input 90 %", FgTuning(true, 0.5f, 0.9f).isValidCustom())
  }
}
