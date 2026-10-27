// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import java.io.BufferedInputStream
import java.io.File
import java.net.InetAddress
import java.net.ServerSocket
import java.net.Socket
import kotlin.concurrent.thread

fun contractFile(path: String): File =
    generateSequence(File(System.getProperty("user.dir")).absoluteFile) { it.parentFile }
        .map { File(it, "contracts/$path") }.first { it.isFile }

/** One recorded request seen by [FakeLink]. */
class Seen(val method: String, val path: String, val contractHeader: String?, val body: String)

/** Tiny plain-HTTP stand-in for the PC (raw sockets, no extra dependency): scripted answers, records requests. */
class FakeLink : AutoCloseable {
    private val server = ServerSocket(0, 8, InetAddress.getByName("127.0.0.1"))
    val seen: MutableList<Seen> = java.util.Collections.synchronizedList(mutableListOf())

    /** status, JSON body, whether to send the `Tatakai-Link: v1` header. */
    @Volatile var handler: (Seen) -> Triple<Int, String, Boolean> = { Triple(200, "{}", true) }

    @Suppress("unused")
    private val acceptor = thread(isDaemon = true) {
        while (!server.isClosed) {
            val socket = try { server.accept() } catch (_: Exception) { break }
            thread(isDaemon = true) { runCatching { serve(socket) } }
        }
    }

    private fun serve(socket: Socket) = socket.use { s ->
        val input = BufferedInputStream(s.getInputStream())
        fun line(): String? {
            val sb = StringBuilder()
            while (true) {
                val c = input.read()
                if (c < 0) return if (sb.isEmpty()) null else sb.toString()
                if (c == '\n'.code) return sb.toString().trimEnd('\r')
                sb.append(c.toChar())
            }
        }
        val request = line() ?: return@use
        val parts = request.split(' ')
        val method = parts[0]
        val target = parts[1]
        var length = 0
        var header: String? = null
        while (true) {
            val l = line() ?: break
            if (l.isEmpty()) break
            val name = l.substringBefore(':').lowercase()
            val value = l.substringAfter(':').trim()
            if (name == "content-length") length = value.toInt()
            if (name == "tatakai-link") header = value
        }
        val body = ByteArray(length)
        var read = 0
        while (read < length) {
            val n = input.read(body, read, length - read)
            if (n < 0) break
            read += n
        }
        val seenReq = Seen(method, target.substringBefore('?'), header, body.decodeToString())
        seen += seenReq
        val (status, json, withHeader) = handler(seenReq)
        val bytes = json.toByteArray()
        val head = buildString {
            append("HTTP/1.1 $status X\r\n")
            if (withHeader) append("Tatakai-Link: v1\r\n")
            append("Content-Type: application/json\r\nConnection: close\r\n")
            append("Content-Length: ${if (status == 204) 0 else bytes.size}\r\n\r\n")
        }
        s.getOutputStream().apply {
            write(head.toByteArray())
            if (status != 204) write(bytes)
            flush()
        }
    }

    val baseUrl: String get() = "http://127.0.0.1:${server.localPort}/"

    override fun close() {
        server.close()
    }
}
