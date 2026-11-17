/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

package app.tatakai.next.framegen

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test

class FgInterpolationRuleTest {
  private val bools = listOf(false, true)

  @Test
  fun `effective full 2x2 table`() {
    val table = mapOf(
      Pair(false, false) to false,
      Pair(false, true) to false,
      Pair(true, false) to true,
      Pair(true, true) to false,
    )
    assertEquals(4, table.size)
    for ((key, expected) in table) {
      val (user, fg) = key
      assertEquals("user=$user fgActive=$fg", expected, FgInterpolationRule.effective(user, fg))
    }
  }

  @Test
  fun `fg active forces interpolation off`() {
    for (user in bools) assertFalse("user=$user", FgInterpolationRule.effective(user, fgActive = true))
  }

  @Test
  fun `turning fg off restores the user value`() {
    val flag = FgActiveFlag()
    for (user in bools) {
      flag.set(true)
      assertFalse(FgInterpolationRule.effective(user, flag.active.value))
      flag.set(false)
      assertEquals("user=$user", user, FgInterpolationRule.effective(user, flag.active.value))
    }
  }
}
