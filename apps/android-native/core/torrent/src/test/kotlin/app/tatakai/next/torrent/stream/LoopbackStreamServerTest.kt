// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.stream

import java.io.ByteArrayOutputStream
import java.io.InputStream
import java.net.Socket
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** Loopback server against an in-memory provider (acceptance criterion of 12 §4 f.5: Range correct, 404 with a wrong token). */
class LoopbackStreamServerTest {

    private val hash = "0123456789abcdef0123456789abcdef01234567"
    private val data = ByteArray(10_000) { (it % 251).toByte() }
    private var opened = 0
    private var closed = 0

    private val provider = object : StreamProvider {
        override fun info(infoHash: String, fileIndex: Int): StreamInfo? =
            if (infoHash == hash && fileIndex == 2) StreamInfo("Show - 01.mkv", data.size.toLong()) else null

        override fun open(infoHash: String, fileIndex: Int): StreamHandle? {
            val info = info(infoHash, fileIndex) ?: return null
            opened++
            return object : StreamHandle {
                override val info = info

                override fun read(position: Long, buffer: ByteArray, offset: Int, length: Int): Int {
                    if (position >= data.size) return -1
                    // deliver in small chunks, like piece boundaries
                    val n = minOf(length, 777, data.size - position.toInt())
                    System.arraycopy(data, position.toInt(), buffer, offset, n)
                    return n
                }

                override fun close() {
                    closed++
                }
            }
        }
    }

    private lateinit var server: LoopbackStreamServer

    @BeforeTest
    fun setUp() {
        server = LoopbackStreamServer(provider, socketTimeoutMs = 5_000)
        server.start()
    }

    @AfterTest
    fun tearDown() = server.close()

    private fun path(token: String = server.token, h: String = hash, index: Int = 2) = "/$token/$h/$index/Show%20-%2001.mkv"

    @Test
    fun `url points to loopback with the token and the encoded name`() {
        val url = server.url(hash, 2, "Show - 01.mkv")
        assertEquals("http://127.0.0.1:${server.port}/${server.token}/$hash/2/Show%20-%2001.mkv", url)
        assertEquals(32, server.token.length)
    }

    @Test
    fun `full GET returns 200 with the whole body`() {
        val r = request("GET", path())
        assertEquals(200, r.code)
        assertEquals("bytes", r.headers["accept-ranges"])
        assertEquals("video/x-matroska", r.headers["content-type"])
        assertEquals(data.size.toString(), r.headers["content-length"])
        assertContentEquals(data, r.body)
        assertEquals(1, opened)
        assertEquals(1, closed)
    }

    @Test
    fun `range request returns 206 with the exact slice`() {
        val r = request("GET", path(), "bytes=100-1099")
        assertEquals(206, r.code)
        assertEquals("bytes 100-1099/${data.size}", r.headers["content-range"])
        assertEquals("1000", r.headers["content-length"])
        assertContentEquals(data.copyOfRange(100, 1100), r.body)
    }

    @Test
    fun `open ended and suffix ranges`() {
        val open = request("GET", path(), "bytes=9000-")
        assertEquals(206, open.code)
        assertEquals("bytes 9000-9999/${data.size}", open.headers["content-range"])
        assertContentEquals(data.copyOfRange(9000, 10_000), open.body)

        val suffix = request("GET", path(), "bytes=-500")
        assertEquals(206, suffix.code)
        assertEquals("bytes 9500-9999/${data.size}", suffix.headers["content-range"])
        assertContentEquals(data.copyOfRange(9500, 10_000), suffix.body)
    }

    @Test
    fun `range past the end is 416`() {
        val r = request("GET", path(), "bytes=20000-")
        assertEquals(416, r.code)
        assertEquals("bytes */${data.size}", r.headers["content-range"])
    }

    @Test
    fun `wrong token is 404 and opens nothing`() {
        val r = request("GET", path(token = "ffffffffffffffffffffffffffffffff"))
        assertEquals(404, r.code)
        assertEquals(0, opened)
    }

    @Test
    fun `unknown file and malformed hash are 404`() {
        assertEquals(404, request("GET", path(index = 7)).code)
        assertEquals(404, request("GET", path(h = "not-a-hash")).code)
        assertEquals(404, request("GET", "/${server.token}").code)
    }

    @Test
    fun `HEAD returns headers without body and without opening a stream`() {
        val r = request("HEAD", path(), "bytes=0-99")
        assertEquals(206, r.code)
        assertEquals("100", r.headers["content-length"])
        assertEquals(0, r.body.size)
        assertEquals(0, opened)
    }

    @Test
    fun `other methods are 405`() {
        assertEquals(405, request("POST", path()).code)
    }

    @Test
    fun `restart issues a new token and the old one stops working`() {
        val old = server.token
        server.stop()
        server.start()
        assertTrue(old != server.token)
        assertEquals(404, request("GET", path(token = old)).code)
        assertEquals(200, request("GET", path()).code)
    }

    private data class Response(val code: Int, val headers: Map<String, String>, val body: ByteArray)

    private fun request(method: String, path: String, range: String? = null): Response {
        Socket("127.0.0.1", server.port).use { socket ->
            socket.soTimeout = 5_000
            val req = buildString {
                append("$method $path HTTP/1.1\r\nHost: 127.0.0.1\r\n")
                if (range != null) append("Range: $range\r\n")
                append("\r\n")
            }
            socket.getOutputStream().write(req.toByteArray(Charsets.ISO_8859_1))
            socket.getOutputStream().flush()
            val input = socket.getInputStream()
            val status = readLine(input)
            val code = status.split(' ')[1].toInt()
            val headers = HashMap<String, String>()
            while (true) {
                val line = readLine(input)
                if (line.isEmpty()) break
                val colon = line.indexOf(':')
                headers[line.substring(0, colon).trim().lowercase()] = line.substring(colon + 1).trim()
            }
            val out = ByteArrayOutputStream()
            if (method != "HEAD") input.copyTo(out)
            return Response(code, headers, out.toByteArray())
        }
    }

    private fun readLine(input: InputStream): String {
        val sb = StringBuilder()
        while (true) {
            val b = input.read()
            if (b < 0 || b == '\n'.code) break
            if (b != '\r'.code) sb.append(b.toChar())
        }
        return sb.toString()
    }
}
