// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.metadata.providers.AliasBatch
import app.tatakai.next.metadata.providers.AniListProvider
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AniListBatchTest {
    private val pageQuery = """
query (${'$'}page: Int, ${'$'}type: MediaType) {
  Page(page: ${'$'}page, perPage: 20) { media(type: ${'$'}type) { ...card } }
}
fragment card on Media { id title { romaji } }"""

    private val mediaQuery = """
query (${'$'}id: Int) {
  Media(id: ${'$'}id) { ...card }
}
fragment card on Media { id title { romaji } }"""

    @Test
    fun parsesSingleFieldOperations() {
        val op = assertNotNull(AliasBatch.parse(pageQuery))
        assertEquals("Page", op.field)
        assertEquals("\$page: Int, \$type: MediaType", op.declarations)
        assertEquals(setOf("card"), op.fragments.keys)
        assertEquals("GenreCollection", AliasBatch.parse("query { GenreCollection }")?.field)
        // two top-level fields, aliased fields and mutations are not batchable
        assertNull(AliasBatch.parse("query { a: Page { id } b: Page { id } }"))
        assertNull(AliasBatch.parse("query { Viewer { id } Page { id } }"))
        assertNull(AliasBatch.parse("mutation { ToggleFollow(userId: 1) { id } }"))
    }

    @Test
    fun buildsOneAliasedQueryWithRenamedVariablesAndSharedFragments() {
        val built = assertNotNull(
            AliasBatch.build(
                listOf(
                    AliasBatch.Part(pageQuery, buildJsonObject { put("page", 1); put("type", "ANIME") }),
                    AliasBatch.Part(mediaQuery, buildJsonObject { put("id", 5114) }),
                    AliasBatch.Part("query { GenreCollection }", JsonObject(emptyMap())),
                ),
            ),
        )
        assertEquals(listOf("q0", "q1", "q2"), built.aliases)
        assertEquals(listOf("Page", "Media", "GenreCollection"), built.fields)
        assertTrue(built.query.startsWith("query (\$q0_page: Int, \$q0_type: MediaType, \$q1_id: Int) {"), built.query)
        assertTrue("q0: Page(page: \$q0_page, perPage: 20) { media(type: \$q0_type) { ...card } }" in built.query, built.query)
        assertTrue("q1: Media(id: \$q1_id) { ...card }" in built.query, built.query)
        assertTrue("q2: GenreCollection" in built.query, built.query)
        assertEquals(1, Regex("fragment card").findAll(built.query).count())
        assertEquals(buildJsonObject { put("q0_page", 1); put("q0_type", "ANIME"); put("q1_id", 5114) }, built.variables)
        // the built query parses as balanced GraphQL again (one operation, three fields)
        assertTrue(built.query.count { it == '{' } == built.query.count { it == '}' })
    }

    @Test
    fun conflictingFragmentsAreNotBatched() {
        val other = mediaQuery.replace("fragment card on Media { id title { romaji } }", "fragment card on Media { id }")
        assertNull(AliasBatch.build(listOf(AliasBatch.Part(pageQuery, JsonObject(emptyMap())), AliasBatch.Part(other, JsonObject(emptyMap())))))
    }

    @Test
    fun splitsTheAnswerBackIntoSingleRequestShapes() {
        val built = assertNotNull(
            AliasBatch.build(listOf(AliasBatch.Part(pageQuery, JsonObject(emptyMap())), AliasBatch.Part(mediaQuery, JsonObject(emptyMap())))),
        )
        val doc = Json.parseToJsonElement(
            """{"data":{"q0":{"media":[{"id":1}]},"q1":null},
               "errors":[{"message":"Not Found.","status":404,"path":["q1"]},{"message":"global"}]}""",
        )
        val (a, b) = AliasBatch.split(doc, built)
        assertEquals(Json.parseToJsonElement("""{"data":{"Page":{"media":[{"id":1}]}},"errors":[{"message":"global"}]}"""), a)
        assertEquals(
            Json.parseToJsonElement("""{"data":{"Media":null},"errors":[{"message":"Not Found.","status":404,"path":["q1"]},{"message":"global"}]}"""),
            b,
        )
        val failed = Json.parseToJsonElement("""{"data":null,"errors":[{"message":"Max query complexity","status":400}]}""")
        assertTrue(AliasBatch.failedAsWhole(failed))
        assertEquals(listOf("""{"data":null,"errors":[{"message":"Max query complexity","status":400}]}"""), AliasBatch.split(failed, built).take(1).map { it.toString() })
    }

    // ------------------------------------------------------------------ through the provider (MockWebServer)

    private val servers = mutableListOf<MockWebServer>()

    @AfterTest
    fun tearDown() = servers.forEach { it.close() }

    private fun server(handler: (RecordedRequest) -> MockResponse): MockWebServer = MockWebServer().also { s ->
        s.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = handler(request)
        }
        s.start()
        servers += s
    }

    private fun ok(body: String) = MockResponse.Builder().code(200).addHeader("Content-Type", "application/json").body(body).build()

    private val trendingPage by lazy { Fixtures.element("anilist-trending-page.json").jsonObject["data"]!!.jsonObject["Page"]!! }

    /** Answers every alias `qN: Page(…)` of a batched query with the trending fixture page. */
    private fun aliasedPages(body: String): String {
        val query = Json.parseToJsonElement(body).jsonObject["query"]!!.jsonPrimitive.content
        val aliases = Regex("""\b(q\d+): Page""").findAll(query).map { it.groupValues[1] }.toList()
        if (aliases.isEmpty()) return """{"data":{"Page":$trendingPage}}"""
        return JsonObject(mapOf("data" to JsonObject(aliases.associateWith { trendingPage }))).toString()
    }

    @Test
    fun concurrentHomeFeedsGoOutAsOneRequestWithIdenticalMapping() = runBlocking<Unit> {
        val requests = AtomicInteger()
        val s = server { req -> requests.incrementAndGet(); ok(aliasedPages(req.body!!.utf8())) }
        val p = AniListProvider(OkHttpClient(), s.url("/").toString().trimEnd('/'))
        val feeds = listOf(FeedKind.TRENDING, FeedKind.AIRING, FeedKind.SEASONAL, FeedKind.POPULAR, FeedKind.TOP_RATED)
        val pages = feeds.map { f -> async { p.feed(MediaKind.ANIME, f, 1) } }.awaitAll()
        assertEquals(1, requests.get())
        val expected = Fixtures.expected("anilist-trending-page.json")
        pages.forEach { assertEquals(expected, Fixtures.encode(it)) }
        val body = s.takeRequest().body!!.utf8()
        assertTrue("q4: Page" in body)
        // only the Trending row (spotlight) asks for the synopsis
        assertTrue("fragment pageCard on" in body && "fragment pageCardLite on" in body)
    }

    @Test
    fun aBatchRefusedAsAWholeIsRetriedPartByPart() = runBlocking<Unit> {
        val requests = AtomicInteger()
        val s = server { req ->
            requests.incrementAndGet()
            val body = req.body!!.utf8()
            if ("q1: Page" in body) {
                MockResponse.Builder().code(400).body("""{"data":null,"errors":[{"message":"Max query complexity","status":400}]}""").build()
            } else {
                ok(aliasedPages(body))
            }
        }
        val p = AniListProvider(OkHttpClient(), s.url("/").toString().trimEnd('/'))
        val pages = listOf(FeedKind.POPULAR, FeedKind.TOP_RATED).map { f -> async { p.feed(MediaKind.ANIME, f, 1) } }.awaitAll()
        assertEquals(3, requests.get())
        pages.forEach { assertEquals(Fixtures.expected("anilist-trending-page.json"), Fixtures.encode(it)) }
    }

    @Test
    fun aRateLimitFailsEveryPartWithTheWait() = runBlocking<Unit> {
        val s = server {
            MockResponse.Builder().code(429).addHeader("Retry-After", "42")
                .body("""{"errors":[{"message":"Too Many Requests.","status":429}],"data":null}""").build()
        }
        val p = AniListProvider(OkHttpClient(), s.url("/").toString().trimEnd('/'))
        val results = listOf(FeedKind.POPULAR, FeedKind.TOP_RATED).map { f -> async { runCatching { p.feed(MediaKind.ANIME, f, 1) } } }.awaitAll()
        results.forEach { r ->
            val e = r.exceptionOrNull() as ProviderException
            assertEquals(429, e.status)
            assertEquals(42_000L, e.retryAfterMs)
        }
        assertEquals(1, s.requestCount)
    }

    @Test
    fun routerCoolsDownForTheRetryAfter() = runBlocking<Unit> {
        val s = server {
            MockResponse.Builder().code(429).addHeader("Retry-After", "120")
                .body("""{"errors":[{"message":"Too Many Requests.","status":429}],"data":null}""").build()
        }
        var now = 5_000L
        val router = MetadataRouter(listOf(AniListProvider(OkHttpClient(), s.url("/").toString().trimEnd('/'))), { listOf("anilist") }, clock = { now })
        val e = assertFailsWith<NoMetadataProviderException> { router.feed(MediaKind.ANIME, FeedKind.TRENDING, 1) }
        assertEquals("anilist", e.rateLimitedProvider)
        assertEquals(120_000L, e.rateLimitedWaitMs)
        assertEquals(5_000L + 120_000, router.statusSnapshot(listOf("anilist")).single().cooldownUntilMs)
        now += 60_001
        assertTrue(router.isCoolingDown("anilist"), "a fixed 60 s would have ended here")
    }

    @Test
    fun identicalConcurrentQueriesShareOneAlias() = runBlocking<Unit> {
        val s = server { req -> ok(aliasedPages(req.body!!.utf8())) }
        val p = AniListProvider(OkHttpClient(), s.url("/").toString().trimEnd('/'))
        val pages = (1..3).map { async { p.feed(MediaKind.ANIME, FeedKind.POPULAR, 1) } }.awaitAll()
        assertEquals(1, s.requestCount)
        assertEquals(1, pages.map { Fixtures.encode(it) }.distinct().size)
        // one alias only: sent as the plain single query
        assertTrue("q0:" !in s.takeRequest().body!!.utf8())
    }

    // ------------------------------------------------------------------ watch order walk

    private fun media(id: Int) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "S$id", titles = emptyMap(), cover = null, banner = null,
        color = null, synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null,
        sourceProvider = "anilist",
    )

    @Test
    fun concurrentWalkMatchesTheSequentialOne() = runBlocking<Unit> {
        // 1 <- 2 <- 3 (self) -> 4 -> 5
        val rel = mapOf(
            1 to listOf(MediaRelation("SEQUEL", media(2))),
            2 to listOf(MediaRelation("PREQUEL", media(1)), MediaRelation("SEQUEL", media(3))),
            3 to listOf(MediaRelation("PREQUEL", media(2)), MediaRelation("SEQUEL", media(4))),
            4 to listOf(MediaRelation("PREQUEL", media(3)), MediaRelation("SEQUEL", media(5))),
            5 to listOf(MediaRelation("PREQUEL", media(4))),
        )
        val hops = AtomicInteger()
        val hop: suspend (Int) -> List<MediaRelation> = { id -> hops.incrementAndGet(); rel[id].orEmpty() }
        val sequential = WatchOrder.walk(media(3), rel.getValue(3), hop)
        val concurrent = WatchOrder.walkConcurrently(media(3), rel.getValue(3), hop)
        assertEquals(listOf(1, 2, 3, 4, 5), sequential.map { it.ref.anilistId })
        assertEquals(sequential, concurrent)
    }
}
