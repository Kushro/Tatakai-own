package app.tatakai.next.metadata

import app.tatakai.next.metadata.providers.AniListProvider
import app.tatakai.next.metadata.providers.AniZipProvider
import app.tatakai.next.metadata.providers.KitsuProvider
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class ProvidersMockServerTest {
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

    @AfterTest
    fun tearDown() {
        servers.forEach { it.close() }
        servers.clear()
    }

    @Test
    fun anilistFeedMapsToExpected() = runBlocking<Unit> {
        val s = server { ok(Fixtures.text("anilist-trending-page.json")) }
        val page = AniListProvider(client, base(s)).feed(MediaKind.ANIME, FeedKind.TRENDING, 1)
        assertEquals(Fixtures.expected("anilist-trending-page.json"), Fixtures.encode(page))
        val req = s.takeRequest()
        assertEquals("POST", req.method)
        assertTrue(req.body!!.utf8().contains("TRENDING_DESC"))
    }

    @Test
    fun anilistSearchMapsToExpected() = runBlocking<Unit> {
        val s = server { ok(Fixtures.text("anilist-search-page.json")) }
        val page = AniListProvider(client, base(s)).search(MediaKind.ANIME, "frieren", 1)
        assertEquals(Fixtures.expected("anilist-search-page.json"), Fixtures.encode(page))
    }

    @Test
    fun anilist429IsTransientProviderFailure() = runBlocking<Unit> {
        val s = server {
            MockResponse.Builder().code(429).addHeader("Retry-After", "60")
                .body("""{"errors":[{"message":"Too Many Requests.","status":429}],"data":null}""").build()
        }
        val e = assertFailsWith<ProviderException> { AniListProvider(client, base(s)).feed(MediaKind.ANIME, FeedKind.TRENDING, 1) }
        assertEquals(429, e.status)
        assertTrue(e.tripsCooldown)
    }

    @Test
    fun aniZipEpisodesMapToExpected() = runBlocking<Unit> {
        val s = server { ok(Fixtures.text("anizip-frieren.json")) }
        val eps = AniZipProvider(client, base(s)).episodes(MediaRef(anilistId = 154587))
        assertEquals(Fixtures.expected("anizip-frieren.json")["episodes"], Fixtures.encodeEpisodes(eps))
        assertEquals("anilist_id=154587", s.takeRequest().url.query)
    }

    @Test
    fun kitsuFeedAndEpisodesMapToExpected() = runBlocking<Unit> {
        val s = server { r ->
            when {
                r.url.encodedPath.endsWith("/episodes") -> ok(Fixtures.text("kitsu-episodes.json"))
                else -> ok(Fixtures.text("kitsu-anime-list.json"))
            }
        }
        val p = KitsuProvider(client, base(s))
        val page = p.feed(MediaKind.ANIME, FeedKind.POPULAR, 1)
        val exp = Fixtures.expected("kitsu-anime-list.json")
        assertEquals(exp["items"], JsonArray(page.items.map(Fixtures::encode)))
        val eps = p.episodes(MediaRef(kitsuId = 46474))
        assertEquals(Fixtures.expected("kitsu-episodes.json")["episodes"], Fixtures.encodeEpisodes(eps))
    }

    @Test
    fun routerFallsBackFromRateLimitedAniListToKitsu() = runBlocking<Unit> {
        val anilist = server { MockResponse.Builder().code(429).body("{}").build() }
        val kitsu = server { ok(Fixtures.text("kitsu-anime-list.json")) }
        var now = 0L
        val router = MetadataRouter(
            listOf(AniListProvider(client, base(anilist)), KitsuProvider(client, base(kitsu))),
            { listOf("anilist", "kitsu") },
            clock = { now },
        )
        val r = router.feed(MediaKind.ANIME, FeedKind.TRENDING, 1)
        assertEquals("kitsu", r.provider)
        assertTrue(r.value.items.isNotEmpty())
        assertEquals(1, anilist.requestCount)

        router.feed(MediaKind.ANIME, FeedKind.TRENDING, 1)
        assertEquals(1, anilist.requestCount, "AniList is skipped during the 60 s cool-down")
        now += 61_000
        router.feed(MediaKind.ANIME, FeedKind.TRENDING, 1)
        assertEquals(2, anilist.requestCount)
        val doc: JsonObject = Fixtures.expected("anilist-rate-limited.json")
        assertEquals(60, (doc["error"] as JsonObject)["cooldownSeconds"].toString().toInt())
    }
}
