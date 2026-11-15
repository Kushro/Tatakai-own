// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.PER_PAGE_AIRING
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

// 0.3.1: a calendar range is one paginated airingSchedules query; pages after the first go out in waves (one aliased request each).
class AiringPageWavesTest {
    @Test
    fun wavesCoverEveryPageOnce() {
        assertEquals(listOf(listOf(1), listOf(2, 3), listOf(4, 5, 6)), AiringLogic.pageWaves(6))
        assertEquals(listOf(listOf(1), listOf(2)), AiringLogic.pageWaves(2))
        assertEquals(listOf(listOf(1)), AiringLogic.pageWaves(1))
    }

    @Test
    fun onlyAClearlyShortPageEndsTheWindow() {
        assertTrue(AiringLogic.endsWindow(0))
        assertTrue(AiringLogic.endsWindow(PER_PAGE_AIRING / 2 - 1))
        assertFalse(AiringLogic.endsWindow(PER_PAGE_AIRING - 3))
    }
}
