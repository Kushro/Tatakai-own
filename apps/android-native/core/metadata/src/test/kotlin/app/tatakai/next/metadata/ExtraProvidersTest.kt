// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.metadata.providers.AniListProvider
import app.tatakai.next.metadata.providers.MalProvider
import app.tatakai.next.metadata.providers.ShikimoriProvider
import app.tatakai.next.metadata.providers.TenraiProvider
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.runBlocking
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Parity wave B (worker r): MAL / Shikimori / Tenrai / AniList airing parsers against recorded fixtures, router status and gap filling. */
class ExtraProvidersTest {
    private val client = OkHttpClient()
    private val servers = mutableListOf<MockWebServer>()

    private fun fixture(name: String): String =
        requireNotNull(javaClass.getResource("/extra/$name")) { "missing fixture $name" }.readText(Charsets.UTF_8)

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
    fun malMediaSendsClientIdAndMaps() = runBlocking<Unit> {
        val s = server { ok(fixture("mal-anime-5114.json")) }
        val m = MalProvider(client, { "abc" }, base(s)).media(MediaKind.ANIME, MediaRef(anilistId = 5114, malId = 5114))
        assertNotNull(m)
        assertEquals(91, m.score)
        assertEquals(64, m.episodes)
        assertEquals("FINISHED", m.status)
        assertEquals("SPRING", m.season)
        assertEquals(2009, m.year)
        assertEquals(5114, m.ref.anilistId)
        assertEquals(listOf("Action", "Adventure"), m.genres)
        assertTrue(m.synopsis!!.startsWith("After a horrific"))
        val req = s.takeRequest()
        assertEquals("abc", req.headers["X-MAL-CLIENT-ID"])
        assertEquals("/anime/5114", req.url.encodedPath)
    }

    @Test
    fun malWithoutClientIdIs401AndNeedsMalId() = runBlocking<Unit> {
        val p = MalProvider(client, { null }, "http://127.0.0.1:1")
        val e = assertFailsWith<ProviderException> { p.media(MediaKind.ANIME, MediaRef(malId = 1)) }
        assertEquals(401, e.status)
        assertNull(MalProvider(client, { "x" }, "http://127.0.0.1:1").media(MediaKind.ANIME, MediaRef(anilistId = 1)))
    }

    @Test
    fun malSearchPage() = runBlocking<Unit> {
        val s = server { ok(fixture("mal-search.json")) }
        val page = MalProvider(client, { "abc" }, base(s)).search(MediaKind.ANIME, "fullmetal", 1)
        assertEquals(2, page.items.size)
        assertTrue(page.hasNext)
        assertEquals(81, page.items[1].score)
    }

    @Test
    fun shikimoriMapsAndDropsCyrillicSynopsis() = runBlocking<Unit> {
        val s = server { ok(fixture("shikimori-anime-5114.json")) }
        val m = ShikimoriProvider(client, base(s)).media(MediaKind.ANIME, MediaRef(malId = 5114))
        assertNotNull(m)
        assertEquals(91, m.score)
        assertNull(m.synopsis)
        assertEquals("SPRING", m.season)
        assertEquals(2009, m.year)
        assertEquals("鋼の錬金術師 FULLMETAL ALCHEMIST", m.titles["native"])
        val req = s.takeRequest()
        assertTrue(req.headers["User-Agent"]!!.startsWith("Tatakai"))
        assertTrue(req.body!!.utf8().contains("\"ids\":\"5114\""))
    }

    @Test
    fun tenraiFullAndPagedEpisodes() = runBlocking<Unit> {
        val s = server { r ->
            val path = r.url.encodedPath
            when {
                path.endsWith("/full") -> ok(fixture("tenrai-anime-5114-full.json"))
                path.endsWith("/episodes") && r.url.queryParameter("page") == "2" -> ok(fixture("tenrai-episodes-p2.json"))
                path.endsWith("/episodes") -> ok(fixture("tenrai-episodes-p1.json"))
                else -> MockResponse.Builder().code(404).build()
            }
        }
        val p = TenraiProvider(client, base(s))
        val m = p.media(MediaKind.ANIME, MediaRef(malId = 5114))
        assertNotNull(m)
        assertEquals(listOf("Action", "Military"), m.genres)
        assertEquals("tenrai", m.sourceProvider)
        val eps = p.episodes(MediaRef(malId = 5114))
        assertEquals(listOf(1, 2, 3), eps.map { it.number })
        assertTrue(eps[2].isFiller)
    }

    @Test
    fun tenrai429TripsCooldownAndShowsInStatus() = runBlocking<Unit> {
        val s = server { MockResponse.Builder().code(429).addHeader("Retry-After", "30").body("{}").build() }
        var now = 1_000L
        val router = MetadataRouter(listOf(TenraiProvider(client, base(s))), { listOf("tenrai") }, clock = { now })
        assertFailsWith<NoMetadataProviderException> { router.feed(MediaKind.ANIME, FeedKind.TRENDING, 1) }
        val st = router.statusSnapshot(listOf("tenrai", "mal")).associateBy { it.id }
        assertTrue(st.getValue("tenrai").enabled)
        assertEquals("HTTP 429", st.getValue("tenrai").lastError)
        assertEquals(1_000L + MetadataRouter.DEFAULT_COOLDOWN_MS, st.getValue("tenrai").cooldownUntilMs)
        assertTrue(!st.getValue("mal").enabled)
        now += MetadataRouter.DEFAULT_COOLDOWN_MS + 1
        assertNull(router.statusSnapshot(listOf("tenrai")).single().cooldownUntilMs)
    }

    @Test
    fun anilistAiringScheduleSortedAndAdultDropped() = runBlocking<Unit> {
        val s = server { ok(fixture("anilist-airing.json")) }
        val (list, hasNext) = AniListProvider(client, base(s)).airingSchedule(1_762_690_000, 1_762_710_000, 1)
        assertEquals(listOf(1, 2), list.map { it.media.ref.anilistId })
        assertEquals(listOf(3, 5), list.map { it.episode })
        assertTrue(!hasNext)
    }

    private fun media(provider: String, score: Int? = null, synopsis: String? = null, malId: Int? = 7) = Media(
        MediaRef(anilistId = 1, malId = malId), MediaKind.ANIME, "T", mapOf("romaji" to "T"), "c", null, null, synopsis,
        listOf("Action"), 12, null, null, null, score, provider,
    )

    private class Fake(override val id: String, val answer: (MediaRef) -> Media?) : MetadataProvider {
        override val kinds = setOf(MediaKind.ANIME)
        override val capabilities = setOf(Capability.MEDIA)
        val refs = mutableListOf<MediaRef>()
        override suspend fun media(kind: MediaKind, ref: MediaRef): Media? { refs += ref; return answer(ref) }
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage = FeedPage(emptyList(), 1, false)
    }

    @Test
    fun routerFillsGapsFromNextEnabledProviders() = runBlocking<Unit> {
        val a = Fake("anilist") { media("anilist") }
        val mal = Fake("mal") { r -> if (r.malId == 7) media("mal", score = 88, synopsis = "From MAL") else null }
        val off = Fake("tenrai") { media("tenrai", score = 1) }
        val router = MetadataRouter(listOf(a, mal, off), { listOf("anilist", "mal") })
        val r = router.media(MediaKind.ANIME, MediaRef(anilistId = 1))
        assertEquals("anilist", r.provider)
        assertEquals(88, r.value!!.score)
        assertEquals("From MAL", r.value!!.synopsis)
        assertEquals(7, mal.refs.single().malId)
        assertTrue(off.refs.isEmpty(), "disabled providers are never asked")
    }

    @Test
    fun fillerFailureNeverFailsTheCall() = runBlocking<Unit> {
        val a = Fake("anilist") { media("anilist") }
        val mal = Fake("mal") { throw ProviderException("mal", 401, "no id") }
        val router = MetadataRouter(listOf(a, mal), { listOf("anilist", "mal") })
        assertEquals("anilist", router.media(MediaKind.ANIME, MediaRef(anilistId = 1)).provider)
        assertTrue(router.isCoolingDown("mal"))
    }
}
