// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import app.tatakai.next.model.MediaSort
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class NovelUiLogicTest {
    @Test
    fun fontFilesAreRecognised() {
        assertTrue(FontFiles.isFont(byteArrayOf(0, 1, 0, 0) + ByteArray(12)))
        assertTrue(FontFiles.isFont("OTTO".toByteArray() + ByteArray(12)))
        assertFalse(FontFiles.isFont("PK\u0003\u0004".toByteArray() + ByteArray(12)))
        assertFalse(FontFiles.isFont(ByteArray(3)))
        assertEquals("My_Font.otf", FontFiles.safeName("My Font!.ttf", "OTTO".toByteArray() + ByteArray(12)))
        assertEquals("font.ttf", FontFiles.safeName("...", byteArrayOf(0, 1, 0, 0)))
    }

    @Test
    fun filtersAlwaysAskForNovels() {
        val q = NovelFilters(query = "overlord", origin = "JP").toQuery()
        assertEquals(listOf("NOVEL"), q.formats)
        assertEquals("JP", q.origin)
        assertEquals(MediaSort.RELEVANCE, q.sort)
        assertTrue(NovelFilters(genre = "Fantasy").active)
        assertFalse(NovelFilters().active)
    }

    @Test
    fun readerFractionAndSpacing() {
        assertEquals(0.06f, dyslexicSpacing("dyslexic"), 0f)
        assertEquals(0f, dyslexicSpacing("serif"), 0f)
        assertEquals(0f, NovelReaderUi().fraction, 0f)
    }
}
