// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import androidx.compose.ui.unit.dp
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** BUG-124: the reader opened on the first paragraph with "Capítulo 1" hidden under the bar, and the bars were big in landscape. */
class NovelReaderLayoutTest {
    @Test
    fun aChapterOpenedAtItsStartShowsItsTitleRow() {
        assertEquals(0, NovelReaderLayout.initialRow(0))
        assertEquals(0, NovelReaderLayout.initialRow(-1))
        assertEquals("paragraph 4 is row 5 (the title row comes first)", 5, NovelReaderLayout.initialRow(4))
    }

    @Test
    fun landscapeBarsAreCompactAndHideOnRotation() {
        assertTrue(NovelReaderLayout.barEstimate(landscape = true) < NovelReaderLayout.barEstimate(landscape = false))
        assertEquals(48.dp, NovelReaderLayout.barEstimate(landscape = true))
        assertTrue(NovelReaderLayout.hideBarsOnRotate(landscape = true))
        assertFalse(NovelReaderLayout.hideBarsOnRotate(landscape = false))
    }
}
