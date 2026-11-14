// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.MANGA_ROUTE_SOURCE
import app.tatakai.next.model.MangaRouteChapter
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** BUG-099: each inner provider of a route-based extension is a reader source of its own, as on desktop. */
class MangaRoutesTest {
    private val toko = CustomSourceRef("tatakai.extension.toko", "Toko", MANGA_ROUTE_SOURCE, "read", "Toko")
    private val answer = listOf(
        MangaRouteChapter("mangadex", "mangadex:a", number = 1.0, lang = "en"),
        MangaRouteChapter("comick", "comick:b", number = 1.0, lang = "es"),
        MangaRouteChapter("mangadex", "mangadex:c", title = "Chapter 2", lang = "en"),
    )

    @Test
    fun chaptersKeepTheKeyAndTheTitleAsItem() {
        val list = MangaRoutes.chapters(toko.extensionId, "105778", answer)
        assertEquals(listOf("manga-route:mangadex", "manga-route:comick", "manga-route:mangadex"), list.map { it.sourceId })
        assertTrue(list.all { it.itemId == "105778" && it.extensionId == toko.extensionId })
        assertEquals("mangadex:c", list[2].id, "manga/pages takes the chapterKey back")
        assertEquals(2.0, list[2].number, "number from the title when the answer has none")
    }

    @Test
    fun oneSourcePerProviderNamedLikeTheAnimeSources() {
        val sources = MangaRoutes.sources(toko, "105778", MangaRoutes.chapters(toko.extensionId, "105778", answer)) { null }
        assertEquals(listOf("Toko · MangaDex", "Toko · Comick"), sources.map { it.extensionName })
        assertEquals(listOf(2, 1), sources.map { it.chapterCount })
        assertEquals(listOf("en"), sources[0].languages)
        assertTrue(sources.all { it.itemId == "105778" })
        assertEquals("Toko · eidos · Weeb Central", MangaRoutes.sourceName("Toko · eidos", "weebcentral"))
        assertEquals("Atsu Comix", MangaRoutes.providerLabel("atsu-comix"))
    }
}
