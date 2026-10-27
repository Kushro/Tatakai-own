package app.tatakai.next.metadata

import app.tatakai.next.model.MediaRef
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** Mapper output must equal contracts/fixtures/metadata/expected.json. */
class MappersFixtureTest {
    @Test
    fun anilistTrendingPage() {
        val page = Mappers.aniListPage(Fixtures.element("anilist-trending-page.json"))
        assertEquals(Fixtures.expected("anilist-trending-page.json"), Fixtures.encode(page))
    }

    @Test
    fun anilistSearchPage() {
        val page = Mappers.aniListPage(Fixtures.element("anilist-search-page.json"))
        assertEquals(Fixtures.expected("anilist-search-page.json"), Fixtures.encode(page))
    }

    @Test
    fun aniZipEpisodesAndIds() {
        val raw = Fixtures.element("anizip-frieren.json") as JsonObject
        val exp = Fixtures.expected("anizip-frieren.json")
        assertEquals(exp["episodes"], Fixtures.encodeEpisodes(Mappers.aniZipEpisodes(raw)))
        val ids = Mappers.aniZipIds(raw)
        assertEquals(
            exp["ids"],
            JsonObject(
                mapOf(
                    "anilistId" to JsonPrimitive(ids.anilistId),
                    "malId" to JsonPrimitive(ids.malId),
                    "kitsuId" to JsonPrimitive(ids.kitsuId),
                ),
            ),
        )
    }

    @Test
    fun kitsuList() {
        val doc = Fixtures.element("kitsu-anime-list.json") as JsonObject
        val page = Mappers.kitsuPage(doc, 1)
        val exp = Fixtures.expected("kitsu-anime-list.json")
        assertEquals(exp["items"], JsonArray(page.items.map(Fixtures::encode)))
        assertEquals(exp["hasNext"], JsonPrimitive(page.hasNext))
    }

    @Test
    fun kitsuEpisodes() {
        val doc = Fixtures.element("kitsu-episodes.json") as JsonObject
        assertEquals(Fixtures.expected("kitsu-episodes.json")["episodes"], Fixtures.encodeEpisodes(Mappers.kitsuEpisodes(doc)))
    }

    @Test
    fun synopsisCleaning() {
        assertEquals("a\nb", Mappers.cleanSynopsis("a<br>b<i></i>\n\n(Source: X)"))
        assertNull(Mappers.cleanSynopsis("  "))
        assertNull(Mappers.cleanSynopsis(null))
        assertEquals(MediaRef(1, null, null), MediaRef(anilistId = 1))
    }
}
