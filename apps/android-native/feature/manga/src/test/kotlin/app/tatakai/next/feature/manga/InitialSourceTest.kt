// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import app.tatakai.next.model.MangaChapterSource
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** BUG-097: a source that did not find the title (HentaiDB in r3) is never preselected. */
class InitialSourceTest {
    private fun src(ext: String, item: String?) = MangaChapterSource(ext, ext, "s", item, emptyList(), null)

    @Test
    fun theFirstMatchedSourceWinsAndNoneWhenNothingMatched() {
        val hentai = src("hentaidb", null)
        val toko = src("toko", "105778")
        assertEquals(toko, initialSource(listOf(hentai, toko), current = null))
        assertNull(initialSource(listOf(hentai, src("latino", null)), current = null))
        assertEquals(toko, initialSource(listOf(src("x", "1"), toko), current = toko), "the current source stays")
        assertEquals(toko, initialSource(listOf(hentai, toko), current = hentai), "not even when it was the current one")
    }
}
