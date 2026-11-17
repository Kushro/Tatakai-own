// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.TranslationProvider
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okhttp3.OkHttpClient
import java.time.ZoneOffset
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue

// 0.4.0 (sub-translation worker): pacing, back-off, Retry-After, the breaker and the keyless Google endpoints (MockWebServer only:
// never the real Google).
class TranslationThrottleTest {
    private lateinit var server: MockWebServer
    private val client = OkHttpClient()

    @BeforeTest
    fun setUp() {
        server = MockWebServer()
        server.start()
    }

    @AfterTest
    fun tearDown() = server.close()

    private fun TestScope.clock() = MonotonicClock { testScheduler.currentTime }

    private fun respond(body: String, code: Int = 200, headers: Map<String, String> = emptyMap()) =
        server.enqueue(MockResponse.Builder().code(code).body(body).apply { headers.forEach { (k, v) -> addHeader(k, v) } }.build())

    @Test
    fun bucketAllowsBurstThenSpacesRequests() = runTest {
        val gate = RequestGate(burst = 2, intervalMs = 1_500, clock = clock())
        gate.acquire()
        gate.acquire()
        assertEquals(0, testScheduler.currentTime)
        gate.acquire()
        assertEquals(1_500, testScheduler.currentTime)
        gate.acquire()
        assertEquals(3_000, testScheduler.currentTime)
        // Idle long enough: the burst is available again.
        testScheduler.advanceTimeBy(10_000)
        val t = testScheduler.currentTime
        gate.acquire()
        gate.acquire()
        assertEquals(t, testScheduler.currentTime)
    }

    @Test
    fun backoffDoublesUpToTheCap() {
        val b = Backoff(baseMs = 2_000, maxMs = 60_000, jitter = 0.0)
        assertEquals(listOf(2_000L, 4_000, 8_000, 16_000, 32_000, 60_000, 60_000), (1..7).map { b.delayMs(it) })
        val jittered = Backoff(jitter = 0.2, random = kotlin.random.Random(1))
        repeat(20) { assertTrue(jittered.delayMs(3) in 6_400..9_600) }
    }

    @Test
    fun retryAfterSecondsAndDate() {
        assertEquals(120_000, RetryAfter.parseMs("120"))
        assertNull(RetryAfter.parseMs(null))
        assertNull(RetryAfter.parseMs("soon"))
        val now = ZonedDateTime.of(2026, 11, 17, 12, 0, 0, 0, ZoneOffset.UTC)
        val header = now.plusSeconds(30).format(DateTimeFormatter.RFC_1123_DATE_TIME)
        assertEquals(30_000, RetryAfter.parseMs(header, now.toInstant().toEpochMilli()))
    }

    @Test
    fun breakerOpensOnRateLimitAndClosesOnSuccess() = runTest {
        val gate = RequestGate(burst = 2, intervalMs = 1_000, clock = clock(), backoff = Backoff(jitter = 0.0))
        assertEquals(2_000, gate.onRateLimited(null))
        val e = assertFailsWith<TranslationException> { gate.acquire() }
        assertEquals(TranslationException.Kind.RATE_LIMITED, e.kind)
        assertEquals(2_000, e.retryAfterMs)
        testScheduler.advanceTimeBy(2_001)
        gate.acquire()
        // Repeated 429s back off longer; Retry-After wins when the server sends it.
        assertEquals(4_000, gate.onRateLimited(null))
        assertEquals(45_000, gate.onRateLimited(45_000))
        assertEquals(3, gate.strikes())
        gate.onSuccess()
        assertEquals(0, gate.cooldownMs())
        assertEquals(0, gate.strikes())
    }

    private fun google(gates: Map<GoogleShape, RequestGate>) = GoogleFreeTranslator(
        client,
        "es",
        listOf(
            GoogleEndpoint(server.url("/translate_a/single").toString(), GoogleShape.SINGLE),
            GoogleEndpoint(server.url("/translate_a/t").toString(), GoogleShape.DICT),
        ),
    ) { gates.getValue(it.shape) }

    @Test
    fun googleFailsOverToDictEndpointOn429AndThenBacksOff() = runTest {
        val gates = mapOf(
            GoogleShape.SINGLE to RequestGate(2, 1_500, clock(), Backoff(jitter = 0.0)),
            GoogleShape.DICT to RequestGate(2, 1_500, clock(), Backoff(jitter = 0.0)),
        )
        val tr = google(gates)
        respond("", code = 429, headers = mapOf("Retry-After" to "30"))
        respond("""[["Hola","en"],["Adiós","en"]]""")
        assertEquals(listOf("Hola", "Adiós"), tr.translate(listOf("Hello", "Bye")))
        val first = server.takeRequest()
        assertEquals("/translate_a/single", first.url.encodedPath)
        assertTrue(first.headers["User-Agent"]!!.startsWith("Mozilla/5.0"))
        assertEquals("GET", first.method) // short batch -> GET
        val second = server.takeRequest()
        assertEquals("dict-chrome-ex", second.url.queryParameter("client"))
        assertEquals(listOf("Hello", "Bye"), second.url.queryParameterValues("q"))
        assertEquals(30_000, gates.getValue(GoogleShape.SINGLE).cooldownMs())

        // The dict endpoint is limited too: nothing is sent while both breakers are open; one RATE_LIMITED with the shortest wait.
        respond("", code = 503)
        val e = assertFailsWith<TranslationException> { tr.translate(listOf("Again")) }
        assertEquals(TranslationException.Kind.RATE_LIMITED, e.kind)
        assertEquals(3, server.requestCount)
        val before = server.requestCount
        val again = assertFailsWith<TranslationException> { tr.translate(listOf("Again")) }
        assertEquals(TranslationException.Kind.RATE_LIMITED, again.kind)
        assertTrue(again.retryAfterMs!! in 1..2_000)
        assertEquals(before, server.requestCount)

        // After the wait the preferred endpoint is used again.
        testScheduler.advanceTimeBy(31_000)
        respond("""[[["Otra vez","Again",null]],null,"en"]""")
        assertEquals(listOf("Otra vez"), tr.translate(listOf("Again")))
        assertEquals("/translate_a/t", server.takeRequest().url.encodedPath) // the 503 one
        assertEquals("/translate_a/single", server.takeRequest().url.encodedPath)
    }

    @Test
    fun googleLongBatchesUsePostAndMismatchesSplitInHalves() = runTest {
        val gates = mapOf(GoogleShape.SINGLE to RequestGate(2, 1_500, clock()), GoogleShape.DICT to RequestGate(2, 1_500, clock()))
        val tr = google(gates)
        val lines = (1..4).map { "Line number $it " + "x".repeat(600) }
        // The joined answer comes back with 3 lines for 4: halves are requested instead of 4 single-line requests.
        respond("""[[["a\nb\nc","",null]],null,"en"]""")
        respond("""[[["L1\n","",null],["L2","",null]],null,"en"]""")
        respond("""[[["L3\nL4","",null]],null,"en"]""")
        assertEquals(listOf("L1", "L2", "L3", "L4"), tr.translate(lines))
        val req = server.takeRequest()
        assertEquals("POST", req.method)
        assertTrue(req.body!!.utf8().startsWith("q="))
        assertEquals(3, server.requestCount)
    }

    @Test
    fun googleDictShapes() {
        assertEquals(listOf("Hola", "Adiós"), GoogleFreeTranslator.parseDict("""[["Hola","en"],["Adiós","en"]]""", 2))
        assertEquals(listOf("Hola", "Adiós"), GoogleFreeTranslator.parseDict("""["Hola","Adiós"]""", 2))
        assertEquals(listOf("Hola"), GoogleFreeTranslator.parseDict("""["Hola","en"]""", 1))
        assertEquals(listOf("Hola"), GoogleFreeTranslator.parseDict("\"Hola\"", 1))
        assertEquals(listOf("Hola mundo"), GoogleFreeTranslator.parseDict("""{"sentences":[{"trans":"Hola ","orig":"Hello "},{"trans":"mundo","orig":"world"}],"src":"en"}""", 1))
        assertEquals(listOf("It's \"ok\""), GoogleFreeTranslator.parseDict("""["It&#39;s &quot;ok&quot;"]""", 1))
        assertNull(GoogleFreeTranslator.parseDict("""[["Hola","en"]]""", 2))
        assertNull(GoogleFreeTranslator.parseDict("<html>", 1))
        assertEquals("Hola\nAdiós", GoogleFreeTranslator.parseSingle("""[[["Hola\n","Hello\n",null,null,10],["Adiós","Bye",null]],null,"en"]"""))
    }

    @Test
    fun keyedProvidersHonourRetryAfter() = runTest {
        val gate = RequestGate(4, 250, clock(), Backoff(jitter = 0.0))
        val tr = SubtitleTranslators.create(TranslatorConfig(TranslationProvider.DEEPL, "es", server.url("/v2/translate").toString(), apiKey = "k:fx"), client) { _, _ -> gate }
        respond("""{"message":"Too many requests"}""", code = 429, headers = mapOf("Retry-After" to "7"))
        val e = assertFailsWith<TranslationException> { tr.translate(listOf("Hi")) }
        assertEquals(TranslationException.Kind.RATE_LIMITED, e.kind)
        assertEquals(7_000, e.retryAfterMs)
        assertEquals(7_000, gate.cooldownMs())
        // DeepL's 456 (quota) is a plain HTTP error, not a back-off.
        testScheduler.advanceTimeBy(8_000)
        respond("""{"message":"quota"}""", code = 456)
        assertEquals(TranslationException.Kind.HTTP, assertFailsWith<TranslationException> { tr.translate(listOf("Hi")) }.kind)
    }
}
