// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.network

import app.tatakai.next.model.RateLimitHeaders
import app.tatakai.next.model.RequestPriority
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

class AniListGatewayMockServerTest {
    private val server = MockWebServer()
    private val calls = AtomicInteger()
    private val slept = mutableListOf<Long>()
    private var now = 1_000_000L

    private fun start(handler: (RecordedRequest) -> MockResponse) {
        server.dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse {
                calls.incrementAndGet()
                return handler(request)
            }
        }
        server.start()
    }

    @AfterTest
    fun stop() = server.close()

    private fun gateway(maxInFlight: Int = 2) = AniListGateway(
        hosts = setOf(server.hostName),
        maxInFlight = maxInFlight,
        clock = { now },
        sleeper = { ms -> synchronized(slept) { slept += ms }; now += ms },
    )

    private fun client(g: AniListGateway) = OkHttpClient.Builder().addInterceptor(g).build()

    private fun post(query: String = "query { GenreCollection }", priority: RequestPriority? = null): Request =
        Request.Builder().url(server.url("/"))
            .post("""{"query":"$query","variables":{}}""".toRequestBody("application/json".toMediaType()))
            .apply { if (priority != null) tag(RequestPriority::class.java, priority) }
            .build()

    private fun ok(body: String = """{"data":{"GenreCollection":["Action"]}}""", remaining: Int = 29) =
        MockResponse.Builder().code(200).addHeader("Content-Type", "application/json")
            .addHeader(RateLimitHeaders.LIMIT, "30").addHeader(RateLimitHeaders.REMAINING, remaining.toString()).body(body).build()

    private fun tooMany(retryAfter: String) = MockResponse.Builder().code(429).addHeader(RateLimitHeaders.RETRY_AFTER, retryAfter)
        .body("""{"errors":[{"message":"Too Many Requests.","status":429}],"data":null}""").build()

    @Test
    fun shortRetryAfterIsWaitedAndRetriedOnce() {
        start { if (calls.get() == 1) tooMany("3") else ok() }
        val g = gateway()
        client(g).newCall(post()).execute().use { r ->
            assertEquals(200, r.code)
            assertTrue("Action" in r.body.string())
        }
        assertEquals(2, calls.get())
        assertEquals(listOf(3_000L), slept)
        assertNotNull(g.snapshot().last429AtMs)
    }

    @Test
    fun aSecondShortOneSurfacesWithTheWait() {
        start { tooMany("2") }
        val g = gateway()
        client(g).newCall(post()).execute().use { r ->
            assertEquals(429, r.code)
            assertEquals("2", r.header(RateLimitHeaders.RETRY_AFTER))
            assertEquals("1", r.header(RateLimitHeaders.GATEWAY))
        }
        assertEquals(2, calls.get())
    }

    @Test
    fun longRetryAfterSurfacesAndTheCooldownAnswersLocally() {
        start { tooMany("120") }
        val g = gateway()
        val c = client(g)
        c.newCall(post()).execute().use { r ->
            assertEquals(429, r.code)
            assertEquals("120", r.header(RateLimitHeaders.RETRY_AFTER))
            assertNull(r.header(AniListGateway.LOCAL_HEADER))
        }
        assertEquals(1, calls.get())
        assertTrue(slept.isEmpty())
        // during the cool-down AniList is not asked again: a local 429 with the remaining wait
        now += 20_000
        c.newCall(post()).execute().use { r ->
            assertEquals(429, r.code)
            assertEquals("100", r.header(RateLimitHeaders.RETRY_AFTER))
            assertEquals("1", r.header(AniListGateway.LOCAL_HEADER))
        }
        assertEquals(1, calls.get())
        assertEquals(1L, g.snapshot().localRateLimited)
    }

    @Test
    fun httpDateRetryAfterIsHonoured() {
        start { if (calls.get() == 1) tooMany("Thu, 01 Jan 1970 00:16:45 GMT") else ok() } // 1_005_000 ms = now + 5 s
        client(gateway()).newCall(post()).execute().use { assertEquals(200, it.code) }
        assertEquals(listOf(5_000L), slept)
    }

    @Test
    fun exhaustedRemainingWaitsForTheReset() {
        // remaining 0, reset in 4 s: the next request sleeps 4 s, then goes
        start { ok(remaining = 0) }
        val g = gateway()
        val c = client(g)
        c.newCall(post()).execute().close()
        g.limiter.onResponse(now, 200, 30, 0, now + 4_000, null)
        c.newCall(post()).execute().use { assertEquals(200, it.code) }
        assertEquals(listOf(4_000L), slept)
        assertEquals(2, calls.get())
    }

    @Test
    fun identicalInFlightQueriesShareOneCall() {
        val release = CountDownLatch(1)
        start {
            release.await(5, TimeUnit.SECONDS)
            ok()
        }
        val g = gateway()
        val c = client(g)
        val pool = Executors.newFixedThreadPool(3)
        val futures = (1..3).map { pool.submit<String> { c.newCall(post()).execute().use { it.body.string() } } }
        // let the followers attach to the leader, then answer
        val deadline = System.currentTimeMillis() + 5_000
        while (g.snapshot().deduped < 2 && System.currentTimeMillis() < deadline) Thread.sleep(10)
        release.countDown()
        futures.forEach { assertTrue("Action" in it.get(5, TimeUnit.SECONDS)) }
        pool.shutdown()
        assertEquals(1, calls.get())
        assertEquals(2L, g.snapshot().deduped)
    }

    @Test
    fun mutationsAreNeverDeduplicated() {
        start { ok() }
        assertNull(AniListGateway.dedupeKey(post(query = "mutation { ToggleFollow(userId: 1) { id } }")))
        assertNotNull(AniListGateway.dedupeKey(post(query = "\\nquery { GenreCollection }")))
        assertTrue(AniListGateway.isMutation("""{"query":"\n mutation (${'$'}id: Int) { X }"}"""))
        assertTrue(AniListGateway.isMutation("not json"))
    }

    @Test
    fun concurrencyIsCapped() {
        val inFlight = AtomicInteger()
        val peak = AtomicInteger()
        start {
            val n = inFlight.incrementAndGet()
            peak.accumulateAndGet(n) { a, b -> maxOf(a, b) }
            Thread.sleep(100)
            inFlight.decrementAndGet()
            ok()
        }
        val c = OkHttpClient.Builder().addInterceptor(gateway(maxInFlight = 2)).build()
        val pool = Executors.newFixedThreadPool(5)
        // different bodies, so nothing is de-duplicated
        val futures = (1..5).map { i -> pool.submit { c.newCall(post(query = "query { Page$i }")).execute().close() } }
        futures.forEach { it.get(10, TimeUnit.SECONDS) }
        pool.shutdown()
        assertEquals(5, calls.get())
        assertTrue(peak.get() <= 2, "peak ${peak.get()}")
    }

    @Test
    fun interactiveGoesBeforeBackground() {
        val order = mutableListOf<String>()
        val first = CountDownLatch(1)
        start { req ->
            val body = req.body?.utf8().orEmpty()
            if ("Blocker" in body) first.await(5, TimeUnit.SECONDS)
            synchronized(order) { order += Regex("""Page\w+""").find(body)?.value.orEmpty() }
            ok()
        }
        val g = gateway(maxInFlight = 1)
        val c = client(g)
        val pool = Executors.newFixedThreadPool(3)
        val blocker = pool.submit { c.newCall(post(query = "query { PageBlocker }")).execute().close() }
        Thread.sleep(150)
        val bg = pool.submit { c.newCall(post(query = "query { PageBackground }", priority = RequestPriority.BACKGROUND)).execute().close() }
        Thread.sleep(150)
        val fg = pool.submit { c.newCall(post(query = "query { PageInteractive }")).execute().close() }
        Thread.sleep(150)
        first.countDown()
        listOf(blocker, bg, fg).forEach { it.get(10, TimeUnit.SECONDS) }
        pool.shutdown()
        assertEquals(listOf("PageBlocker", "PageInteractive", "PageBackground"), order)
    }

    @Test
    fun otherHostsPassThrough() {
        start { ok() }
        val g = AniListGateway(hosts = setOf("graphql.anilist.co"), clock = { now })
        client(g).newCall(post()).execute().close()
        assertEquals(0L, g.snapshot().total)
    }
}
