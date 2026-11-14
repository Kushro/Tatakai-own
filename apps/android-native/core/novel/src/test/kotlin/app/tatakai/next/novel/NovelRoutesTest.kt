// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.MangaRouteChapter
import app.tatakai.next.model.NOVEL_ROUTE_SOURCE
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class NovelRoutesTest {
    @Test
    fun oneSourcePerProviderWithTheChapterKeyAsId() {
        val toko = CustomSourceRef("tatakai.extension.toko", "Toko", NOVEL_ROUTE_SOURCE, "novel", "Toko")
        val chapters = NovelRoutes.chapters(
            toko.extensionId, "105778",
            listOf(
                MangaRouteChapter("wuxiaworld", "wuxiaworld:1", number = 1.0, lang = "en", group = "WW"),
                MangaRouteChapter("novelfire", "novelfire:1", number = 1.0, lang = "en"),
                MangaRouteChapter("wuxiaworld", "wuxiaworld:2", number = 2.0, lang = "en", group = "WW"),
            ),
        )
        assertTrue(chapters.all { it.itemId == "105778" && NovelRoutes.isRouteSource(it.sourceId) })
        val sources = NovelRoutes.sources(toko, "105778", chapters)
        assertEquals(listOf("Toko · Wuxiaworld", "Toko · Novelfire"), sources.map { it.extensionName })
        assertEquals(listOf(2, 1), sources.map { it.chapterCount })
        assertEquals("WW", sources[0].translator)
        assertEquals("wuxiaworld:2", chapters[2].id)
    }

    @Test
    fun englishTitleGoesFirstLikeTheDesktop() {
        assertEquals(
            listOf("Mushoku Tensei: Jobless Reincarnation", "Mushoku Tensei: Isekai Ittara Honki Dasu", "無職転生", "Mushoku"),
            NovelRoutes.searchTitles(
                shown = "Mushoku Tensei: Isekai Ittara Honki Dasu",
                english = "Mushoku Tensei: Jobless Reincarnation",
                romaji = "Mushoku Tensei: Isekai Ittara Honki Dasu",
                native = "無職転生",
                synonyms = listOf("Mushoku", " "),
            ),
        )
        assertEquals(listOf("Overlord"), NovelRoutes.searchTitles("Overlord", null, "Overlord", null, emptyList()))
    }

    /** BUG-123: "Toko · Atsu Novel@cmt4…" twice → "Toko · Atsu Novel · Red" / "… · Red 2" (the group tells them apart). */
    @Test
    fun twoAtsuNovelSourcesAreNamedByTheirGroup() {
        val toko = CustomSourceRef("tatakai.extension.toko", "Toko", NOVEL_ROUTE_SOURCE, "novel", "Toko")
        val chapters = NovelRoutes.chapters(
            toko.extensionId, "85470",
            listOf(
                MangaRouteChapter("atsu-novel@cmt4ng1ye0001j7odsjhaatbz", "x:1", number = 1.0, lang = "en", group = "Red"),
                MangaRouteChapter("atsu-novel@cmt4ssu9w0000csodgkdy427f", "y:1", number = 1.0, lang = "en", group = "Red 2"),
            ),
        )
        val sources = NovelRoutes.sources(toko, "85470", chapters)
        assertEquals(listOf("Toko · Atsu Novel · Red", "Toko · Atsu Novel · Red 2"), sources.map { it.extensionName })
    }
}
