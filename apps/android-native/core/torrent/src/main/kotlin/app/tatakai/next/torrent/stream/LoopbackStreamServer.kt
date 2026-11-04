// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.stream

import java.io.BufferedInputStream
import java.io.BufferedOutputStream
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
import java.util.Locale
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicInteger

/**
 * Minimal HTTP/1.1 server for torrent streaming (docs/redesign/12 §4 f.2, same scheme as desktop tk-torrent `stream.rs`):
 * - listens on `127.0.0.1` only, on a free port;
 * - path `/<token>/<infoHash>/<fileIndex>/<name>`, where `token` is a random 128-bit hex string generated on every [start];
 * - `GET` / `HEAD`, `Range: bytes=` -> 206 with `Content-Range`, `Accept-Ranges: bytes`, `Content-Type` by extension;
 * - 404 for a wrong token, a malformed path or an unknown torrent / file; 405 for other methods; 416 for unsatisfiable ranges;
 * - one request per connection (`Connection: close`): players open one connection per range anyway.
 * Reads block inside [StreamHandle.read] until the piece is downloaded (sequential download + deadlines, see TorrentRepositoryImpl).
 */
internal class LoopbackStreamServer(
    private val provider: StreamProvider,
    private val socketTimeoutMs: Int = 90_000,
) : Closeable {

    @Volatile private var serverSocket: ServerSocket? = null

    /** The current per-start token; empty while stopped. */
    @Volatile var token: String = ""
        private set

    private val threadIds = AtomicInteger()
    private val executor: ExecutorService = Executors.newCachedThreadPool { r ->
        Thread(r, "tatakai-torrent-http-${threadIds.incrementAndGet()}").apply { isDaemon = true }
    }

    val isRunning: Boolean get() = serverSocket?.isClosed == false

    /** Port of the running server, -1 when stopped. */
    val port: Int get() = serverSocket?.takeIf { !it.isClosed }?.localPort ?: -1

    /** Starts listening (idempotent while running); every fresh start gets a new token. Returns the port. */
    @Synchronized
    fun start(): Int {
        serverSocket?.takeIf { !it.isClosed }?.let { return it.localPort }
        token = newToken()
        val socket = ServerSocket(0, BACKLOG, InetAddress.getByName(LOOPBACK))
        serverSocket = socket
        executor.execute { acceptLoop(socket) }
        return socket.localPort
    }

    /** Stops listening; open responses end when their sockets close. The server can be started again (new token). */
    @Synchronized
    fun stop() {
        runCatching { serverSocket?.close() }
        serverSocket = null
        token = ""
    }

    override fun close() {
        stop()
        executor.shutdownNow()
    }

    /** `http://127.0.0.1:<port>/<token>/<infoHash>/<fileIndex>/<name>`; call after [start]. */
    fun url(infoHash: String, fileIndex: Int, name: String): String =
        "http://$LOOPBACK:$port/$token/$infoHash/$fileIndex/${encodePathSegment(name.ifBlank { "video" })}"

    private fun acceptLoop(socket: ServerSocket) {
        while (!socket.isClosed) {
            val client = try {
                socket.accept()
            } catch (_: IOException) {
                break
            }
            try {
                executor.execute { handle(client) }
            } catch (_: Exception) {
                runCatching { client.close() }
            }
        }
    }

    private fun handle(client: Socket) {
        client.use { c ->
            try {
                c.soTimeout = socketTimeoutMs
                c.tcpNoDelay = true
                val input = BufferedInputStream(c.getInputStream())
                val output = BufferedOutputStream(c.getOutputStream(), BUFFER_SIZE)
                val request = readRequest(input)
                if (request == null) {
                    writeStatus(output, 400, "Bad Request")
                    return
                }
                serve(request, output)
            } catch (_: SocketException) {
                // client went away
            } catch (_: IOException) {
                // timeout or broken pipe: nothing to report to a player that is gone
            }
        }
    }

    private fun serve(request: Request, out: OutputStream) {
        val method = request.method.uppercase(Locale.ROOT)
        val target = parseTarget(request.path)
        if (target == null) {
            writeStatus(out, 404, "Not Found")
            return
        }
        if (method != "GET" && method != "HEAD") {
            writeStatus(out, 405, "Method Not Allowed", extra = listOf("Allow" to "GET, HEAD"))
            return
        }
        val info = provider.info(target.infoHash, target.fileIndex)
        if (info == null) {
            writeStatus(out, 404, "Not Found")
            return
        }
        val size = info.size
        val contentType = contentTypeFor(info.name)
        when (val range = parseRange(request.headers["range"], size)) {
            RangeRequest.Unsatisfiable -> {
                writeStatus(out, 416, "Range Not Satisfiable", extra = listOf("Content-Range" to "bytes */$size"))
            }
            RangeRequest.Full -> respond(out, method, target, 200, "OK", contentType, ByteRange(0, size - 1), size, partial = false)
            is RangeRequest.Partial -> respond(out, method, target, 206, "Partial Content", contentType, range.range, size, partial = true)
        }
    }

    private fun respond(
        out: OutputStream,
        method: String,
        target: Target,
        code: Int,
        reason: String,
        contentType: String,
        range: ByteRange,
        size: Long,
        partial: Boolean,
    ) {
        val length = if (size <= 0) 0 else range.length
        val headers = buildList {
            add("Content-Type" to contentType)
            add("Accept-Ranges" to "bytes")
            add("Content-Length" to length.toString())
            if (partial) add("Content-Range" to "bytes ${range.start}-${range.endInclusive}/$size")
            add("Cache-Control" to "no-store")
            add("Connection" to "close")
        }
        if (method == "HEAD" || length == 0L) {
            writeHead(out, code, reason, headers)
            out.flush()
            return
        }
        val handle = provider.open(target.infoHash, target.fileIndex)
        if (handle == null) {
            writeStatus(out, 404, "Not Found")
            return
        }
        handle.use { h ->
            writeHead(out, code, reason, headers)
            val buffer = ByteArray(BUFFER_SIZE)
            var position = range.start
            val end = range.endInclusive + 1
            while (position < end) {
                val want = minOf(buffer.size.toLong(), end - position).toInt()
                val read = h.read(position, buffer, 0, want)
                if (read <= 0) break // premature end: the client sees a short body and retries
                out.write(buffer, 0, read)
                position += read
            }
            out.flush()
        }
    }

    private fun parseTarget(rawPath: String): Target? {
        val path = rawPath.substringBefore('?').substringBefore('#')
        val segments = path.split('/')
        // ["", token, infoHash, fileIndex, name...]
        if (segments.size < 4 || segments[0].isNotEmpty()) return null
        val current = token
        if (current.isEmpty() || !constantTimeEquals(segments[1], current)) return null
        val infoHash = segments[2].lowercase(Locale.ROOT)
        if (!INFO_HASH.matches(infoHash)) return null
        val fileIndex = segments[3].toIntOrNull()?.takeIf { it >= 0 } ?: return null
        return Target(infoHash, fileIndex)
    }

    private fun readRequest(input: InputStream): Request? {
        val requestLine = readLine(input) ?: return null
        val parts = requestLine.split(' ')
        if (parts.size < 3 || !parts[2].startsWith("HTTP/")) return null
        val headers = HashMap<String, String>()
        var total = requestLine.length
        while (true) {
            val line = readLine(input) ?: return null
            if (line.isEmpty()) break
            total += line.length
            if (total > MAX_HEADER_BYTES) return null
            val colon = line.indexOf(':')
            if (colon <= 0) continue
            headers[line.substring(0, colon).trim().lowercase(Locale.ROOT)] = line.substring(colon + 1).trim()
        }
        return Request(parts[0], parts[1], headers)
    }

    /** Reads one CRLF (or LF) terminated ISO-8859-1 line; null at end of stream or when too long. */
    private fun readLine(input: InputStream): String? {
        val sb = StringBuilder()
        while (true) {
            val b = input.read()
            if (b < 0) return if (sb.isEmpty()) null else sb.toString()
            if (b == '\n'.code) {
                if (sb.isNotEmpty() && sb[sb.length - 1] == '\r') sb.setLength(sb.length - 1)
                return sb.toString()
            }
            sb.append(b.toChar())
            if (sb.length > MAX_HEADER_BYTES) return null
        }
    }

    private fun writeStatus(out: OutputStream, code: Int, reason: String, extra: List<Pair<String, String>> = emptyList()) {
        writeHead(out, code, reason, extra + listOf("Content-Length" to "0", "Connection" to "close"))
        out.flush()
    }

    private fun writeHead(out: OutputStream, code: Int, reason: String, headers: List<Pair<String, String>>) {
        val sb = StringBuilder("HTTP/1.1 ").append(code).append(' ').append(reason).append("\r\n")
        headers.forEach { (k, v) -> sb.append(k).append(": ").append(v).append("\r\n") }
        sb.append("\r\n")
        out.write(sb.toString().toByteArray(Charsets.ISO_8859_1))
    }

    private data class Request(val method: String, val path: String, val headers: Map<String, String>)

    private data class Target(val infoHash: String, val fileIndex: Int)

    companion object {
        const val LOOPBACK: String = "127.0.0.1"
        private const val BACKLOG = 50
        private const val BUFFER_SIZE = 64 * 1024
        private const val MAX_HEADER_BYTES = 16 * 1024
        private val INFO_HASH = Regex("[0-9a-f]{40}")
        private val random = SecureRandom()

        /** 128 random bits as 32 lowercase hex characters. */
        internal fun newToken(): String {
            val bytes = ByteArray(16)
            random.nextBytes(bytes)
            return bytes.joinToString("") { "%02x".format(it.toInt() and 0xff) }
        }

        private fun constantTimeEquals(a: String, b: String): Boolean =
            MessageDigest.isEqual(a.toByteArray(Charsets.UTF_8), b.toByteArray(Charsets.UTF_8))
    }
}
