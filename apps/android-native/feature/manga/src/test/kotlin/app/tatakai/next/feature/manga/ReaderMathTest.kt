// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import app.tatakai.next.model.MediaSort
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReaderMathTest {
    @Test
    fun prefetchAtSeventyPercent() {
        assertFalse(ReaderMath.shouldPrefetch(5, 10)) // 6 / 10
        assertTrue(ReaderMath.shouldPrefetch(6, 10)) // 7 / 10
        assertFalse(ReaderMath.shouldPrefetch(0, 0))
    }

    @Test
    fun spreadsKeepTheCoverAlone() {
        assertEquals(listOf(listOf(0), listOf(1, 2), listOf(3, 4), listOf(5)), ReaderMath.spreads(6, doublePage = true))
        assertEquals(listOf(listOf(0), listOf(1), listOf(2)), ReaderMath.spreads(3, doublePage = false))
        assertTrue(ReaderMath.spreads(0, doublePage = true).isEmpty())
        val s = ReaderMath.spreads(6, true)
        assertEquals(2, ReaderMath.spreadOf(s, 4))
        assertEquals(0, ReaderMath.spreadOf(s, 99))
    }

    @Test
    fun warmRangeEagerOrLazy() {
        assertEquals(0 until 20, ReaderMath.warmRange(3, 20, eager = true, ahead = 5))
        assertEquals(4 until 9, ReaderMath.warmRange(3, 20, eager = false, ahead = 5))
        assertEquals(20 until 20, ReaderMath.warmRange(19, 20, eager = false, ahead = 5))
    }

    @Test
    fun borderCropFindsContent() {
        val w = 100
        val h = 100
        val white = 0xFFFFFFFF.toInt()
        val black = 0xFF000000.toInt()
        val pixels = IntArray(w * h) { white }
        for (y in 20 until 80) for (x in 10 until 90) pixels[y * w + x] = black
        assertArrayEquals(intArrayOf(6, 16, 94, 84), BorderCrop.bounds(pixels, w, h))
        // No margin worth cropping.
        val full = IntArray(w * h) { i -> if (i == 0) white else black }
        assertNull(BorderCrop.bounds(full, w, h))
        // Blank page.
        assertNull(BorderCrop.bounds(IntArray(w * h) { white }, w, h))
    }

    @Test
    fun filtersBuildTheQuery() {
        val f = MangaFilters(query = " berserk ", origin = "JP", status = "FINISHED", genre = "Action", year = 1989)
        assertTrue(f.active)
        val q = f.toQuery()
        assertEquals("berserk", q.query)
        assertEquals("JP", q.origin)
        assertEquals(listOf("Action"), q.genres)
        assertEquals(1989, q.yearFrom)
        assertEquals(1989, q.yearTo)
        assertEquals(MediaSort.RELEVANCE, q.sort)
        assertEquals(MediaSort.SCORE, MangaFilters(origin = "KR", sort = MediaSort.SCORE).toQuery().sort)
        assertFalse(MangaFilters().active)
    }
}
