// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class MangaParsersTest {
    private fun fixture(name: String): String = requireNotNull(javaClass.classLoader?.getResource(name)) { name }.readText()

    @Test
    fun mangaUpdatesSearchAndSeries() {
        val hits = MangaUpdatesClient.parseSearch(ReaderJson.parseToJsonElement(fixture("mangaupdates_search.json")))
        assertEquals(2, hits.size)
        assertEquals(55099564912L to "Berserk", hits.first())

        val series = MangaUpdatesClient.parseSeries(ReaderJson.parseToJsonElement(fixture("mangaupdates_series.json")) as JsonObject)
        assertNotNull(series)
        series!!
        assertEquals("Berserk", series.title)
        assertEquals(listOf("Hakusensha", "Dark Horse"), series.publishers) // Original publisher first
        assertEquals("Young Animal, Animal House", series.serialization)
        assertEquals("42 Volumes (Ongoing)", series.status)
        assertEquals(90, series.rating)
        assertEquals(1989, series.year)
        assertEquals(376, series.latestChapter)
        assertTrue(series.licensed)
        assertFalse(series.completed)
    }

    @Test
    fun mangaDexPickByAniListLinkThenTitle() {
        val doc = ReaderJson.parseToJsonElement(fixture("mangadex_manga.json"))
        assertEquals("801513ba-a712-498c-8f57-cae55b38cc92", MangaDexClient.pickManga(doc, 30002, listOf("Berserk")))
        // No AniList link match: the closest title wins.
        assertEquals("801513ba-a712-498c-8f57-cae55b38cc92", MangaDexClient.pickManga(doc, null, listOf("Berserk")))
        assertNull(MangaDexClient.pickManga(doc, null, listOf("One Piece")))
    }

    @Test
    fun mangaDexFeedWithLanguageAndGroup() {
        val chapters = MangaDexClient.parseFeed(ReaderJson.parseToJsonElement(fixture("mangadex_feed.json")), "m1")
        // The external chapter without pages is dropped.
        assertEquals(2, chapters.size)
        val en = chapters.first()
        assertEquals("ch-1", en.id)
        assertEquals(1.0, en.number!!, 0.0)
        assertEquals(1, en.volume)
        assertEquals("en", en.lang)
        assertEquals("Evil Genius", en.group)
        assertEquals(MangaDexClient.EXTENSION_ID, en.extensionId)
        assertNotNull(en.releasedAtMs)
        assertEquals("es", chapters[1].lang)
        assertEquals("Lector Scans", chapters[1].group)
    }

    @Test
    fun mangaDexAtHomePages() {
        val pages = MangaDexClient.parseAtHome(ReaderJson.parseToJsonElement(fixture("mangadex_athome.json")))
        assertEquals(2, pages.size)
        assertEquals("https://uploads.mangadex.org/data/abc123/1-a.png", pages[0].url)
        assertEquals(1, pages[1].index)
    }

    @Test
    fun aniListDetailsWithAdaptation() {
        val m = (ReaderJson.parseToJsonElement(fixture("anilist_manga_details.json")) as JsonObject).sub("data").sub("Media")!!
        val info = AniListMangaApi.parseInfo(m)
        assertEquals(30002, info.media.ref.anilistId)
        assertEquals(MediaKind.MANGA, info.media.kind)
        assertEquals("JP", info.origin)
        assertEquals("RELEASING", info.status)
        assertEquals("Berserk", info.media.title)
        assertTrue(info.media.synopsis!!.startsWith("His name is Guts."))
        assertFalse(info.media.synopsis!!.contains("<"))
        val adaptation = info.relations.first { it.relation == "ADAPTATION" }
        assertEquals(MediaKind.ANIME, adaptation.media.kind)
        assertEquals(33, adaptation.media.ref.anilistId)
        assertEquals("Guts", info.characters.first().character.name)
        assertEquals(1989, info.media.year)
    }

    @Test
    fun searchVariablesCarryMangaFilters() {
        val vars = AniListMangaApi.searchVariables(
            MediaSearchQuery(query = "berserk", status = "finished", origin = "jp", genres = listOf("Action"), yearFrom = 1989, sort = MediaSort.SCORE),
            page = 2,
            perPage = 30,
            novels = false,
        )
        assertEquals("berserk", (vars["search"] as JsonPrimitive).content)
        assertEquals("FINISHED", (vars["status"] as JsonPrimitive).content)
        assertEquals("JP", (vars["countryOfOrigin"] as JsonPrimitive).content)
        assertEquals("NOVEL", ((vars["format_not_in"] as JsonArray)[0] as JsonPrimitive).content)
        assertEquals(19889999, (vars["startDate_greater"] as JsonPrimitive).content.toInt())
        val novels = AniListMangaApi.searchVariables(MediaSearchQuery(), 1, 30, novels = true)
        assertEquals("NOVEL", ((novels["format_in"] as JsonArray)[0] as JsonPrimitive).content)
        assertNull(novels["format_not_in"])
    }

    @Test
    fun synopsisCleanup() {
        assertEquals("A\n\nB \"c\"", AniListMangaApi.cleanSynopsis("A<br><br><br>B <i>&quot;c&quot;</i>"))
        assertNull(AniListMangaApi.cleanSynopsis("  "))
    }
}
