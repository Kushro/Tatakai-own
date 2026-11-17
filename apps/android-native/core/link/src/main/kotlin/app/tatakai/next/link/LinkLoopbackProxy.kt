// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import java.io.BufferedInputStream
import java.io.Closeable
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import java.net.SocketException
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.concurrent.ExecutorService
import java.util.concurrent.SynchronousQueue
import java.util.concurrent.ThreadPoolExecutor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger

/**
 * Tatakai Next 0.4.0: the PC's Link HLS on libmpv. The PC only serves a phone that presents its client certificate (mTLS with
 * the pinned CA), which the OkHttp client of [LinkCrypto] does and Media3 uses directly. libmpv opens URLs itself (FFmpeg), and
 * handing it the certificate would mean writing the decrypted private key to a file (Enigmata's Temple does that, with a 0600
 * temporary file); this phone's key stays sealed in memory instead, and this proxy terminates the mTLS on the phone:
 *
 * - it listens on the loopback interface only (`127.0.0.1`, random port) and answers only paths that start with its
 *   [token] (128 random bits): another app on the phone cannot borrow the session without it;
 * - it forwards only `GET` / `HEAD` of session files (`/v1/sessions/<uuid>/<file>`: playlists, init and segments), never the
 *   rest of the API, through the mTLS client to the paired PC (`Range` forwarded);
 * - the PC answers `503 segment_wait_timeout` + `Retry-After` while its engine is still making a segment and `409
 *   session_not_ready` while it prepares: Media3 retries those (`RemoteLoadErrorPolicy`), FFmpeg's HLS demuxer would skip the
 *   segment or stop, so the proxy waits and asks again (up to [retryBudgetMs]) and FFmpeg just sees a slow answer;
 * - one request per connection (`Connection: close`), bodies streamed, nothing cached or logged (no URLs in logcat).
 *
 * [upstream] gives the paired PC's base URL and mTLS client (null when unpaired: 502).
 */
class LinkLoopbackProxy(
    private val upstream: () -> Pair<HttpUrl, OkHttpClient>?,
    private val retryBudgetMs: Long = RETRY_BUDGET_MS,
    private val sleep: (Long) -> Unit = { Thread.sleep(it) },
) : Closeable {
    /** Path prefix every request must carry (hex, 32 chars). */
    val token: String = ByteArray(16).also { SecureRandom().nextBytes(it) }.joinToString("") { "%02x".format(it) }

    @Volatile private var server: ServerSocket? = null
    private var workers: ExecutorService? = null

    /** The loopback URL of [remoteUrl] (a `playbackUrl` of the paired PC), starting the proxy if needed; null = not a session file URL. */
    @Synchronized
    fun localUrl(remoteUrl: String): String? {
        val remote = remoteUrl.toHttpUrlOrNull() ?: return null
        val path = remote.encodedPath
        if (!isSessionPath(path)) return null
        val port = ensureStarted() ?: return null
        return "http://127.0.0.1:$port/$token$path"
    }

    /** Port the proxy listens on (starting it), null when it cannot listen. */
    @Synchronized
    fun ensureStarted(): Int? {
        server?.takeIf { !it.isClosed }?.let { return it.localPort }
        val socket = runCatching { ServerSocket(0, BACKLOG, InetAddress.getByName("127.0.0.1")) }.getOrNull() ?: return null
        val n = AtomicInteger()
        // bounded: FFmpeg opens a few connections at a time (playlist + segment + prefetch); more waits in the backlog
        val pool = ThreadPoolExecutor(0, MAX_WORKERS, 30, TimeUnit.SECONDS, SynchronousQueue()) { r ->
            Thread(r, "link-loopback-${n.incrementAndGet()}").apply { isDaemon = true }
        }
        server = socket
        workers = pool
        Thread({ acceptLoop(socket, pool) }, "link-loopback-accept").apply { isDaemon = true }.start()
        return socket.localPort
    }

    override fun close() {
        synchronized(this) {
            runCatching { server?.close() }
            server = null
            workers?.shutdownNow()
            workers = null
        }
    }

    private fun acceptLoop(socket: ServerSocket, pool: ExecutorService) {
        while (!socket.isClosed) {
            val client = try {
                socket.accept()
            } catch (_: IOException) {
                break
            }
            try {
                pool.execute { client.use { serve(it) } }
            } catch (_: Exception) {
                // all workers busy (or shutting down): refuse this one, FFmpeg reconnects
                runCatching { client.use { respondError(it.getOutputStream(), 503, "Service Unavailable") } }
            }
        }
    }

    private fun serve(socket: Socket) {
        socket.soTimeout = HEAD_TIMEOUT_MS
        val input = BufferedInputStream(socket.getInputStream())
        val out = socket.getOutputStream()
        val head = readHead(input) ?: return
        val parsed = parseRequest(head, token)
        if (parsed == null) {
            respondError(out, 404, "Not Found")
            return
        }
        val target = upstream()
        if (target == null) {
            respondError(out, 502, "Bad Gateway")
            return
        }
        socket.soTimeout = 0
        val (base, client) = target
        val url = base.newBuilder().encodedPath(parsed.path).build()
        val response = try {
            fetch(client, url, parsed)
        } catch (_: IOException) {
            respondError(out, 502, "Bad Gateway")
            return
        }
        response.use { relay(it, parsed.method == "HEAD", out) }
    }

    /** Asks the PC, waiting through `503 + Retry-After` / `409 session_not_ready` up to [retryBudgetMs]. */
    private fun fetch(client: OkHttpClient, url: HttpUrl, req: ProxyRequest): Response {
        var waited = 0L
        while (true) {
            val builder = Request.Builder().url(url).header("Accept", "*/*")
            req.range?.let { builder.header("Range", it) }
            if (req.method == "HEAD") builder.head() else builder.get()
            val response = client.newCall(builder.build()).execute()
            val wait = retryDelayMs(response.code, response.header("Retry-After"))
            if (wait == null || waited + wait > retryBudgetMs) return response
            response.close()
            sleep(wait)
            waited += wait
        }
    }

    private fun relay(response: Response, headOnly: Boolean, out: OutputStream) {
        val sb = StringBuilder()
        sb.append("HTTP/1.1 ").append(response.code).append(' ').append(response.message.ifBlank { "OK" }).append("\r\n")
        for (name in FORWARDED_HEADERS) response.header(name)?.let { sb.append(name).append(": ").append(it).append("\r\n") }
        val length = response.body.contentLength()
        if (length >= 0) sb.append("Content-Length: ").append(length).append("\r\n")
        sb.append("Connection: close\r\n\r\n")
        try {
            out.write(sb.toString().toByteArray(Charsets.ISO_8859_1))
            if (!headOnly) response.body.byteStream().use { copy(it, out) }
            out.flush()
        } catch (_: SocketException) {
            // FFmpeg closed the connection (seek, stop): nothing to do
        } catch (_: IOException) {
            // the PC's stream broke mid-body: closing makes FFmpeg retry or fail the segment
        }
    }

    private fun copy(from: InputStream, to: OutputStream) {
        val buf = ByteArray(COPY_BUFFER)
        while (true) {
            val n = from.read(buf)
            if (n < 0) return
            to.write(buf, 0, n)
        }
    }

    internal data class ProxyRequest(val method: String, val path: String, val range: String?)

    internal companion object {
        const val RETRY_BUDGET_MS = 30_000L
        const val MAX_RETRY_DELAY_MS = 2_000L
        const val HEAD_TIMEOUT_MS = 10_000
        const val MAX_HEAD_BYTES = 16 * 1024
        const val MAX_WORKERS = 8
        const val BACKLOG = 16
        const val COPY_BUFFER = 64 * 1024
        private val FORWARDED_HEADERS = listOf("Content-Type", "Content-Range", "Accept-Ranges", "Cache-Control", "Retry-After")
        private val SESSION_PATH = Regex("""^/v1/sessions/[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}/[A-Za-z0-9_.-]{1,64}$""")

        /** Only session files go through the proxy (no `..`, no other API route). */
        fun isSessionPath(path: String): Boolean = SESSION_PATH.matches(path) && ".." !in path

        /**
         * The request of [head] when it is `GET` / `HEAD` of `/<token>/v1/sessions/<id>/<file>` (query ignored); null otherwise.
         * The token is compared in constant time.
         */
        fun parseRequest(head: String, token: String): ProxyRequest? {
            val lines = head.split("\r\n")
            val parts = lines.firstOrNull()?.split(' ') ?: return null
            if (parts.size != 3 || !parts[2].startsWith("HTTP/1.")) return null
            val method = parts[0]
            if (method != "GET" && method != "HEAD") return null
            val target = parts[1].substringBefore('?')
            val prefix = "/$token"
            val given = target.take(prefix.length)
            if (!MessageDigest.isEqual(given.toByteArray(), prefix.toByteArray())) return null
            val path = target.substring(prefix.length)
            if (!isSessionPath(path)) return null
            val range = lines.drop(1).firstOrNull { it.startsWith("range:", ignoreCase = true) }
                ?.substringAfter(':')?.trim()?.takeIf { RANGE.matches(it) }
            return ProxyRequest(method, path, range)
        }

        private val RANGE = Regex("""^bytes=\d*-\d*$""")

        /** How long to wait before asking the PC again, or null to pass the answer on as it is. */
        fun retryDelayMs(code: Int, retryAfter: String?): Long? = when (code) {
            503 -> retryAfter?.trim()?.toLongOrNull()?.let { (it * 1000).coerceIn(250, MAX_RETRY_DELAY_MS) }
            409 -> 500L // session_not_ready: the PC prepares the first segments (2-10 s)
            else -> null
        }

        /** The request head (up to the blank line), null when the client sent nothing usable in time. */
        fun readHead(input: InputStream): String? {
            val bytes = java.io.ByteArrayOutputStream()
            var matched = 0
            val end = byteArrayOf('\r'.code.toByte(), '\n'.code.toByte(), '\r'.code.toByte(), '\n'.code.toByte())
            try {
                while (bytes.size() < MAX_HEAD_BYTES) {
                    val b = input.read()
                    if (b < 0) return null
                    bytes.write(b)
                    matched = if (b.toByte() == end[matched]) matched + 1 else if (b.toByte() == end[0]) 1 else 0
                    if (matched == end.size) return bytes.toString(Charsets.ISO_8859_1.name()).removeSuffix("\r\n\r\n")
                }
            } catch (_: IOException) {
                return null
            }
            return null
        }

        fun respondError(out: OutputStream, code: Int, message: String) {
            runCatching {
                out.write("HTTP/1.1 $code $message\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".toByteArray(Charsets.ISO_8859_1))
                out.flush()
            }
        }
    }
}
