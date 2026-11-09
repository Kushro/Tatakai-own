// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.metadata.providers.AniListProvider
import app.tatakai.next.metadata.providers.AniListQueries
import app.tatakai.next.metadata.providers.KitsuProvider
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DiscoverProviderMockServerTest {
    private val client = OkHttpClient()
    private val servers = mutableListOf<MockWebServer>()

    private fun server(handler: (RecordedRequest) -> MockResponse): MockWebServer {
        val s = MockWebServer()
        s.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = handler(request)
        }
        s.start()
        servers += s
        return s
    }

    private fun base(s: MockWebServer) = s.url("/").toString().trimEnd('/')

    private fun ok(body: String) = MockResponse.Builder().code(200).addHeader("Content-Type", "application/json").body(body).build()

    private fun variables(req: RecordedRequest): JsonObject =
        Json.parseToJsonElement(req.body!!.utf8()).jsonObject["variables"]!!.jsonObject

    @AfterTest
    fun tearDown() {
        servers.forEach { it.close() }
        servers.clear()
    }

    @Test
    fun advancedVariablesCarryEveryFilter() {
        val q = MediaSearchQuery(
            query = " frieren ", genres = listOf("Fantasy"), tags = listOf("Elf"), formats = listOf("tv"), status = "FINISHED",
            season = "FALL", yearFrom = 2010, yearTo = 2015, episodesFrom = 1, episodesTo = 13, scoreFrom = 80, scoreTo = 100,
            sort = MediaSort.SCORE,
        )
        val v = AniListQueries.advancedVariables(MediaKind.ANIME, q, 2, 20, currentYear = 2026)
        assertEquals(JsonPrimitive("frieren"), v["search"])
        assertEquals(JsonArray(listOf(JsonPrimitive("Fantasy"))), v["genre_in"])
        assertEquals(JsonArray(listOf(JsonPrimitive("Elf"))), v["tag_in"])
        assertEquals(JsonArray(listOf(JsonPrimitive("TV"))), v["format_in"])
        assertEquals(JsonPrimitive("FINISHED"), v["status"])
        assertEquals(JsonPrimitive("FALL"), v["season"])
        assertEquals(JsonPrimitive(2026), v["seasonYear"])
        assertEquals(JsonPrimitive(20099999), v["startDate_greater"])
        assertEquals(JsonPrimitive(20160000), v["startDate_lesser"])
        assertEquals(JsonPrimitive(0), v["episodes_greater"])
        assertEquals(JsonPrimitive(14), v["episodes_lesser"])
        assertEquals(JsonPrimitive(79), v["averageScore_greater"])
        assertEquals(JsonPrimitive(101), v["averageScore_lesser"])
        assertEquals(JsonArray(listOf(JsonPrimitive("SCORE_DESC"))), v["sort"])
        assertEquals(JsonPrimitive(false), v["isAdult"])
        assertEquals(JsonPrimitive(2), v["page"])
    }

    @Test
    fun adultOnSendsNoAdultFilterAndRelevanceNeedsText() {
        val v = AniListQueries.advancedVariables(MediaKind.ANIME, MediaSearchQuery(adult = true, sort = MediaSort.RELEVANCE), 1, 20, 2026)
        assertNull(v["isAdult"])
        assertNull(v["search"])
        assertEquals(JsonArray(listOf(JsonPrimitive("POPULARITY_DESC"))), v["sort"])
        val t = AniListQueries.advancedVariables(MediaKind.ANIME, MediaSearchQuery(query = "x", sort = MediaSort.RELEVANCE), 1, 20, 2026)
        assertEquals(JsonArray(listOf(JsonPrimitive("SEARCH_MATCH"))), t["sort"])
    }

    @Test
    fun searchAdvancedPostsTheQueryAndPagesWithHasNext() = runBlocking<Unit> {
        val s = server { ok(Fixtures.text("anilist-trending-page.json")) }
        val page = AniListProvider(client, base(s)).searchAdvanced(
            MediaKind.ANIME, MediaSearchQuery(query = "frieren", genres = listOf("Fantasy"), formats = listOf("TV")), 1,
        )
        assertTrue(page.hasNext)
        assertEquals(154587, page.items.first().ref.anilistId)
        val req = s.takeRequest()
        val body = req.body!!.utf8()
        assertTrue(body.contains("genre_in"))
        val v = variables(req)
        assertEquals(JsonArray(listOf(JsonPrimitive("Fantasy"))), v["genre_in"])
        assertEquals(JsonArray(listOf(JsonPrimitive("TV"))), v["format_in"])
    }

    @Test
    fun studioSearchUsesTheStudioQuery() = runBlocking<Unit> {
        val s = server {
            ok(
                """{"data":{"Studio":{"id":11,"name":"Madhouse","media":{"pageInfo":{"currentPage":1,"hasNextPage":true},"nodes":[
                  {"id":154587,"idMal":52991,"type":"ANIME","title":{"romaji":"Sousou no Frieren"},"isAdult":false,"genres":[]},
                  {"id":5,"type":"MANGA","title":{"romaji":"Some manga"},"isAdult":false,"genres":[]},
                  {"id":6,"type":"ANIME","title":{"romaji":"Adult"},"isAdult":true,"genres":[]}]}}}}""",
            )
        }
        val page = AniListProvider(client, base(s)).searchAdvanced(MediaKind.ANIME, MediaSearchQuery(studio = "Madhouse"), 1)
        assertEquals(listOf(154587), page.items.map { it.ref.anilistId })
        assertTrue(page.hasNext)
        val req = s.takeRequest()
        assertTrue(req.body!!.utf8().contains("Studio(search:"))
        assertEquals(JsonPrimitive("Madhouse"), variables(req)["search"])
    }

    @Test
    fun detailsRelationsAndRecommendationsShareOneRequest() = runBlocking<Unit> {
        val s = server { ok(Fixtures.text("anilist-details-frieren.json")) }
        val p = AniListProvider(client, base(s))
        val ref = MediaRef(anilistId = 154587)
        assertEquals(listOf("Madhouse"), p.details(ref)?.studios)
        assertEquals(3, p.relations(ref).size)
        assertEquals(1, p.recommendations(ref).size)
        assertEquals(1, s.requestCount)
    }

    @Test
    fun routerDiscoverSkipsProvidersWithoutTheCapability() = runBlocking<Unit> {
        val anilist = server { ok(Fixtures.text("anilist-details-frieren.json")) }
        val kitsu = server { ok("{}") }
        val router = MetadataRouter(
            listOf(KitsuProvider(client, base(kitsu)), AniListProvider(client, base(anilist))),
            { listOf("kitsu", "anilist") },
        )
        val r = router.discover(MediaKind.ANIME, { it == null }) { it.details(MediaRef(anilistId = 154587)) }
        assertEquals("anilist", r.provider)
        assertEquals(0, kitsu.requestCount)

        val onlyKitsu = MetadataRouter(listOf(KitsuProvider(client, base(kitsu))), { listOf("kitsu") })
        val e = assertFailsWith<NoMetadataProviderException> {
            onlyKitsu.discover(MediaKind.ANIME, { it == null }) { it.details(MediaRef(anilistId = 1)) }
        }
        assertTrue(e.errors.isEmpty(), "no DISCOVER provider = not available, not a failure")
    }

    @Test
    fun traceMoeUploadsMultipartAndParses() = runBlocking<Unit> {
        val s = server { ok(Fixtures.text("tracemoe-search.json")) }
        val matches = TraceMoeClient(client, base(s)).search(byteArrayOf(1, 2, 3), "image/png")
        assertEquals(3, matches.size)
        val req = s.takeRequest()
        assertEquals("POST", req.method)
        assertEquals("/search", req.url.encodedPath)
        assertEquals("anilistInfo", req.url.encodedQuery)
        assertTrue(req.headers["Content-Type"]!!.startsWith("multipart/form-data"))
        assertTrue(req.body!!.utf8().contains("name=\"image\""))
    }

    @Test
    fun traceMoeQuotaIsAProviderFailure() = runBlocking<Unit> {
        val s = server { MockResponse.Builder().code(402).body("""{"error":"Search quota depleted"}""").build() }
        val e = assertFailsWith<ProviderException> { TraceMoeClient(client, base(s)).search(byteArrayOf(1), "image/jpeg") }
        assertEquals(402, e.status)
        assertFalse(e.status == 0)
    }
}
