// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import java.net.InetAddress
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 0.4.0: the PC's Link HLS reaches libmpv through a loopback proxy that terminates the mTLS (and only serves session files). */
class LinkLoopbackProxyTest {
    private val sid = "6f1d3c1e-8d0f-4b6a-9a76-0c2a8d9d2f11"
    private val token = "0123456789abcdef0123456789abcdef"
    private var server: MockWebServer? = null
    private var proxy: LinkLoopbackProxy? = null

    @AfterTest
    fun tearDown() {
        proxy?.close()
        server?.close()
    }

    private fun pc(handler: (RecordedRequest) -> MockResponse): String {
        val s = MockWebServer().apply {
            dispatcher = object : Dispatcher() {
                override fun dispatch(request: RecordedRequest): MockResponse = handler(request)
            }
            start(InetAddress.getByName("127.0.0.1"), 0)
        }
        server = s
        return "http://127.0.0.1:${s.port}"
    }

    private fun proxyFor(origin: String, sleeps: MutableList<Long> = mutableListOf()): LinkLoopbackProxy =
        LinkLoopbackProxy(upstream = { origin.toHttpUrl() to OkHttpClient() }, sleep = { sleeps += it }).also { proxy = it }

    private fun get(url: String, range: String? = null) =
        OkHttpClient().newCall(Request.Builder().url(url).apply { range?.let { header("Range", it) } }.build()).execute()

    @Test
    fun onlyGetAndHeadOfSessionFilesWithTheTokenPass() {
        val ok = LinkLoopbackProxy.parseRequest("GET /$token/v1/sessions/$sid/master.m3u8?x=1 HTTP/1.1\r\nHost: 127.0.0.1", token)
        assertEquals(LinkLoopbackProxy.ProxyRequest("GET", "/v1/sessions/$sid/master.m3u8", null), ok)
        val ranged = LinkLoopbackProxy.parseRequest("HEAD /$token/v1/sessions/$sid/seg_3.m4s HTTP/1.1\r\nRange: bytes=0-99", token)
        assertEquals("bytes=0-99", ranged?.range)
        assertEquals("HEAD", ranged?.method)
        for (bad in listOf(
            "GET /ffffffffffffffffffffffffffffffff/v1/sessions/$sid/master.m3u8 HTTP/1.1", // wrong token
            "GET /v1/sessions/$sid/master.m3u8 HTTP/1.1", // no token
            "POST /$token/v1/sessions/$sid/position HTTP/1.1", // not a read
            "GET /$token/v1/capabilities HTTP/1.1", // not a session file
            "GET /$token/v1/sessions/$sid/../../devices/self HTTP/1.1",
            "GET /$token/v1/sessions/not-a-uuid/master.m3u8 HTTP/1.1",
            "GET /$token/v1/sessions/$sid/master.m3u8",
        )) {
            assertNull(LinkLoopbackProxy.parseRequest(bad, token), bad)
        }
        // a Range header that is not a plain byte range is dropped, not forwarded
        assertNull(LinkLoopbackProxy.parseRequest("GET /$token/v1/sessions/$sid/seg_1.m4s HTTP/1.1\r\nRange: items=1", token)?.range)
    }

    @Test
    fun retriesOnlyWhatThePcAsksToRetry() {
        assertEquals(1000L, LinkLoopbackProxy.retryDelayMs(503, "1"))
        assertEquals(LinkLoopbackProxy.MAX_RETRY_DELAY_MS, LinkLoopbackProxy.retryDelayMs(503, "30"))
        assertNull(LinkLoopbackProxy.retryDelayMs(503, null), "server_busy without Retry-After goes to the player")
        assertEquals(500L, LinkLoopbackProxy.retryDelayMs(409, null))
        for (code in listOf(200, 206, 404, 410, 500)) assertNull(LinkLoopbackProxy.retryDelayMs(code, "1"))
    }

    @Test
    fun playlistsAndSegmentsAreRelayedAndASlowSegmentIsWaitedFor() {
        val segmentCalls = AtomicInteger()
        val ranges = java.util.Collections.synchronizedList(mutableListOf<String?>())
        val origin = pc { req ->
            val path = req.url.encodedPath
            if (path.endsWith("/seg_2.m4s")) ranges += req.headers["Range"]
            when {
                path.endsWith("/master.m3u8") ->
                    MockResponse.Builder().code(200).addHeader("Content-Type", "application/vnd.apple.mpegurl").body("#EXTM3U\nmedia.m3u8\n").build()
                path.endsWith("/seg_2.m4s") && segmentCalls.incrementAndGet() < 3 ->
                    MockResponse.Builder().code(503).addHeader("Retry-After", "1").body("""{"error":{"code":"segment_wait_timeout"}}""").build()
                path.endsWith("/seg_2.m4s") ->
                    MockResponse.Builder().code(206).addHeader("Content-Range", "bytes 0-3/4").body("SEG2").build()
                else -> MockResponse.Builder().code(410).body("""{"error":{"code":"session_gone"}}""").build()
            }
        }
        val sleeps = mutableListOf<Long>()
        val p = proxyFor(origin, sleeps)
        val local = assertNotNull(p.localUrl("https://192.168.1.20:47321/v1/sessions/$sid/master.m3u8"))
        assertTrue(local.startsWith("http://127.0.0.1:") && "/${p.token}/v1/sessions/$sid/master.m3u8" in local, local)
        get(local).use { r ->
            assertEquals(200, r.code)
            assertEquals("#EXTM3U\nmedia.m3u8\n", r.body.string())
            assertEquals("application/vnd.apple.mpegurl", r.header("Content-Type"))
        }
        // the relative segment URL resolves against the proxy; two 503 + Retry-After are waited out
        get(local.replace("master.m3u8", "seg_2.m4s"), range = "bytes=0-3").use { r ->
            assertEquals(206, r.code)
            assertEquals("SEG2", r.body.string())
            assertEquals("bytes 0-3/4", r.header("Content-Range"))
        }
        assertEquals(listOf(1000L, 1000L), sleeps)
        assertEquals(listOf<String?>("bytes=0-3", "bytes=0-3", "bytes=0-3"), ranges.toList())
        // a session the PC no longer has goes to the player as is (it renews it)
        get(local.replace("master.m3u8", "media.m3u8")).use { assertEquals(410, it.code) }
        // without the token nothing is served
        get(local.replace(p.token, "f".repeat(32))).use { assertEquals(404, it.code) }
    }

    @Test
    fun notASessionUrlOrNoPairingGivesNothing() {
        val p = LinkLoopbackProxy(upstream = { null }).also { proxy = it }
        assertNull(p.localUrl("https://pc:1/v1/capabilities"))
        val local = assertNotNull(p.localUrl("https://pc:1/v1/sessions/$sid/master.m3u8"))
        get(local).use { assertEquals(502, it.code) }
    }
}
