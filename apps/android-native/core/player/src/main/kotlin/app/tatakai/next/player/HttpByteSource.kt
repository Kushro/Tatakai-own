// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.Call
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import java.io.IOException
import java.io.InputStream
import kotlin.math.min

// 0.4.0 (sub-translation worker): [MediaByteSource] over HTTP Range requests, for [EmbeddedSubtitleExtractor]. Used for remote files
// (debrid / direct links: remote = true, paced and budgeted by the extractor) and for the torrent's loopback stream (remote = false,
// [availability] = the downloaded byte ranges). Every request is bounded to one chunk inside an available range, so the loopback
// server is never asked for a piece the torrent does not have (that would make it wait for, and prioritise, that piece).

/** Parses `Content-Range: bytes 0-0/12345` -> 12345 (null for `*` or garbage). */
internal fun contentRangeTotal(header: String?): Long? = header?.substringAfterLast('/')?.trim()?.toLongOrNull()

class HttpByteSource(
    client: OkHttpClient,
    private val url: String,
    private val headers: Map<String, String>,
    override val remote: Boolean,
    /** Downloaded ranges given the file length (torrent); null = everything is reachable. */
    private val availability: ((length: Long) -> List<LongRange>?)? = null,
) : MediaByteSource {
    // No call timeout: a chunk of a slow remote host may take a while; the extractor's coroutine cancels it.
    private val http = client.newBuilder().callTimeout(0, java.util.concurrent.TimeUnit.MILLISECONDS).build()

    @Volatile
    override var length: Long = -1L
        private set

    private val calls = HashSet<Call>()

    override suspend fun prepare() = withContext(Dispatchers.IO) {
        if (length >= 0) return@withContext
        execute(0, 0).use { res ->
            when {
                res.code == 206 -> length = contentRangeTotal(res.header("Content-Range")) ?: throw IOException("no Content-Range")
                // Ranges are required: resyncing / jumping needs random access, and reading from 0 every time is not an option.
                res.isSuccessful -> throw IOException("server ignores Range")
                else -> throw IOException("HTTP ${res.code}")
            }
        }
    }

    override fun available(): List<LongRange>? = availability?.invoke(length)

    private fun execute(from: Long, toInclusive: Long): Response {
        val req = Request.Builder().url(url).apply { headers.forEach { (k, v) -> header(k, v) } }
            .header("Range", "bytes=$from-$toInclusive")
            .header("Accept-Encoding", "identity")
            .build()
        val call = http.newCall(req)
        synchronized(this) { calls += call }
        val res = try {
            call.execute()
        } finally {
            synchronized(this) { calls -= call }
        }
        if (res.code != 206 && !(res.isSuccessful && from == 0L)) {
            res.close()
            throw IOException("HTTP ${res.code}")
        }
        return res
    }

    /** End (inclusive) of the next request from [pos]: one chunk, never past the available range nor the file. */
    private fun chunkEnd(pos: Long): Long {
        var end = pos + CHUNK - 1
        if (length > 0) end = min(end, length - 1)
        available()?.firstOrNull { pos in it }?.let { end = min(end, it.last) } ?: if (availability != null) throw DataNotAvailableException(pos) else Unit
        return end
    }

    override fun openAt(position: Long): RawReader = object : RawReader() {
        private var pos = position
        private var res: Response? = null
        private var stream: InputStream? = null
        private var left = 0L

        override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
            if (this@HttpByteSource.length in 0..pos) return -1
            if (stream == null || left <= 0) {
                close()
                val end = chunkEnd(pos)
                val r = execute(pos, end)
                res = r
                stream = r.body.byteStream()
                left = end - pos + 1
            }
            val n = stream!!.read(buffer, offset, min(length.toLong(), left).toInt())
            if (n < 0) {
                close()
                return -1
            }
            pos += n
            left -= n
            return n
        }

        override fun close() {
            runCatching { res?.close() }
            res = null
            stream = null
            left = 0
        }
    }

    override fun readAt(position: Long, buffer: ByteArray, offset: Int, length: Int): Int {
        if (length <= 0) return 0
        if (this.length in 0..position) return -1
        var end = position + length - 1
        if (this.length > 0) end = min(end, this.length - 1)
        available()?.let { ranges -> ranges.firstOrNull { position in it }?.let { end = min(end, it.last) } ?: return 0 }
        execute(position, end).use { res ->
            val input = res.body.byteStream()
            var got = 0
            val want = (end - position + 1).toInt()
            while (got < want) {
                val n = input.read(buffer, offset + got, want - got)
                if (n < 0) break
                got += n
            }
            return got
        }
    }

    override fun close() {
        synchronized(this) { calls.toList() }.forEach { runCatching { it.cancel() } }
    }

    companion object {
        private const val CHUNK = 4L * 1024 * 1024
    }
}
