// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaTag
import app.tatakai.next.model.NextAiring
import app.tatakai.next.model.Trailer
import kotlinx.serialization.json.Json
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Parity wave A (worker o): AniList details / relations / characters / character / trends and trace.moe fixtures -> core model. */
class DiscoverMappersTest {

    @Test
    fun detailsFactsOfFrieren() {
        val d = assertNotNull(DiscoverMappers.details(Fixtures.element("anilist-details-frieren.json")))
        assertEquals(MediaRef(anilistId = 154587, malId = 52991), d.ref)
        assertEquals("TV", d.format)
        assertEquals("MANGA", d.source)
        assertEquals(24, d.durationMin)
        assertEquals("2023-09-29", d.startDate)
        assertEquals("2024-03", d.endDate)
        assertEquals(91, d.meanScore)
        assertEquals(listOf("Madhouse"), d.studios)
        assertEquals(listOf("Aniplex", "TOHO animation"), d.producers)
        assertEquals("JP", d.countryOfOrigin)
        // Tags by relevance, spoilers flagged.
        assertEquals(listOf("Elf", "Travel", "Time Skip"), d.tags.map { it.name })
        assertEquals(MediaTag("Time Skip", 70, isSpoiler = true, category = "Theme-Other"), d.tags.last())
        assertEquals(listOf("Frieren at the Funeral", "Frieren: Beyond Journey's End"), d.synonyms)
        assertEquals(Trailer("youtube", "qgQqf2VKEKo", "https://i.ytimg.com/vi/qgQqf2VKEKo/hqdefault.jpg"), d.trailer)
        assertEquals("https://www.youtube.com/watch?v=qgQqf2VKEKo", d.trailer?.url)
        assertEquals(NextAiring(5, 1767225600), d.nextAiring)
        // Only http(s) links survive.
        assertEquals(setOf("Crunchyroll", "Official Site"), d.externalLinks.keys)
    }

    @Test
    fun relationsKeepTypeAndDropEmptyNodes() {
        val r = DiscoverMappers.relations(Fixtures.element("anilist-details-frieren.json"))
        assertEquals(listOf("SOURCE", "SEQUEL", "SIDE_STORY"), r.map { it.relation })
        assertEquals(MediaKind.MANGA, r[0].media.kind)
        assertEquals(118586, r[0].media.ref.anilistId)
        assertEquals("Sousou no Frieren 2nd Season", r[1].media.title)
        assertEquals("https://s4.anilist.co/file/anilistcdn/media/anime/cover/medium/bx170068.jpg", r[2].media.cover)
    }

    @Test
    fun recommendationsSkipNullNodes() {
        val r = DiscoverMappers.recommendations(Fixtures.element("anilist-details-frieren.json"))
        assertEquals(1, r.size)
        assertEquals(21, r[0].media.ref.anilistId)
        assertEquals(812, r[0].rating)
    }

    @Test
    fun charactersPageWithVoiceActorsPerLanguage() {
        val p = DiscoverMappers.charactersPage(Fixtures.element("anilist-characters-frieren.json"), 1)
        assertTrue(p.hasNext)
        assertEquals(1, p.page)
        assertEquals(listOf("Frieren", "Himmel"), p.items.map { it.character.name })
        val frieren = p.items[0]
        assertEquals("MAIN", frieren.role)
        assertEquals(listOf("JAPANESE", "ENGLISH", "SPANISH"), frieren.voiceActors.map { it.language })
        assertEquals("Atsumi Tanezaki", frieren.voiceActors.first().name)
        assertEquals("https://s4.anilist.co/file/anilistcdn/staff/medium/n106622.png", frieren.voiceActors[1].image)
        assertEquals("https://s4.anilist.co/file/anilistcdn/character/medium/b182234.png", p.items[1].character.image)
    }

    @Test
    fun characterPage() {
        val c = assertNotNull(DiscoverMappers.character(Fixtures.element("anilist-character-frieren.json")))
        assertEquals(176754, c.id)
        assertEquals("Frieren", c.name)
        assertEquals("フリーレン", c.nativeName)
        assertEquals("1000+", c.age)
        assertEquals("Female", c.gender)
        assertNull(c.birthday)
        assertEquals(45210, c.favourites)
        // Markdown flattened, spoiler removed.
        assertEquals("Race: Elf\n\nA mage who was part of the hero Himmel's party.", c.description)
        assertEquals(listOf(154587, 170068), c.appearances.map { it.media.ref.anilistId })
        assertEquals("MAIN", c.appearances[0].role)
        // Voice actors de-duplicated across appearances.
        assertEquals(listOf("Atsumi Tanezaki", "Mallorie Rodak"), c.voiceActors.map { it.name })
    }

    @Test
    fun birthdayFormats() {
        val json = Json
        assertEquals("10-31", DiscoverMappers.birthday(json.parseToJsonElement("""{"year":null,"month":10,"day":31}""") as kotlinx.serialization.json.JsonObject))
        assertEquals("1999-02-03", DiscoverMappers.birthday(json.parseToJsonElement("""{"year":1999,"month":2,"day":3}""") as kotlinx.serialization.json.JsonObject))
        assertNull(DiscoverMappers.birthday(null))
    }

    @Test
    fun trendsAreSummedPerMediaAndAdultDropped() {
        val page = DiscoverMappers.trendsPage(Fixtures.element("anilist-trends-week.json"), 1, adult = false)
        assertEquals(listOf(154587, 21), page.items.map { it.ref.anilistId })
        assertTrue(page.hasNext)
        val withAdult = DiscoverMappers.trendsPage(Fixtures.element("anilist-trends-week.json"), 1, adult = true)
        assertEquals(listOf(999, 154587, 21), withAdult.items.map { it.ref.anilistId })
    }

    @Test
    fun traceMoeResults() {
        val doc = Fixtures.element("tracemoe-search.json")
        assertNull(DiscoverMappers.traceMoeError(doc))
        val r = DiscoverMappers.traceMoe(doc)
        assertEquals(3, r.size)
        val first = r[0]
        assertEquals(MediaRef(anilistId = 154587, malId = 52991), first.ref)
        assertEquals("Frieren: Beyond Journey's End", first.title)
        assertEquals(1, first.episode)
        assertEquals(663.17, first.fromSec)
        assertEquals(665.42, first.toSec)
        assertEquals(0.9637, first.similarity)
        assertTrue(first.previewVideo!!.startsWith("https://api.trace.moe/video/"))
        assertEquals("Sousou no Frieren: ●● no Mahou", r[1].title)
        assertEquals(3, r[1].episode)
        // Bare id without anilistInfo: falls back to the file name.
        assertEquals(21, r[2].ref.anilistId)
        assertEquals("One Piece 1000.mp4", r[2].title)
        assertNull(r[2].episode)
    }

    @Test
    fun traceMoeErrorField() {
        val doc = Json.parseToJsonElement("""{"error":"Search queue is full","result":[]}""")
        assertEquals("Search queue is full", DiscoverMappers.traceMoeError(doc))
        assertEquals(emptyList(), DiscoverMappers.traceMoe(doc))
    }

    @Test
    fun genresAndTagCatalogue() {
        val genres = DiscoverMappers.genres(Json.parseToJsonElement("""{"data":{"GenreCollection":["Action","Hentai",""]}}"""))
        assertEquals(listOf("Action", "Hentai"), genres)
        val tags = DiscoverMappers.tags(
            Json.parseToJsonElement(
                """{"data":{"MediaTagCollection":[
                  {"name":"Time Skip","category":"Theme","isGeneralSpoiler":true,"isAdult":false},
                  {"name":"Nudity","category":"Sexual","isGeneralSpoiler":false,"isAdult":true},
                  {"name":"Elf","category":"Cast","isGeneralSpoiler":false,"isAdult":false}]}}""",
            ),
        )
        assertEquals(listOf("Elf", "Time Skip"), tags.map { it.name })
        assertTrue(tags.last().isSpoiler)
    }
}
