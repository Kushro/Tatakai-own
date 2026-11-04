// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.stream

import java.util.Locale

// Pure helpers of the loopback streaming server (docs/redesign/12 §4 f.2, 10 §2.2): `Range` parsing and content types.
// No Android dependency so the JVM tests can exercise them directly.

/** Inclusive byte range `[start, endInclusive]`. */
internal data class ByteRange(val start: Long, val endInclusive: Long) {
    val length: Long get() = endInclusive - start + 1
}

/** What a `Range` request header asks for, resolved against the file size. */
internal sealed interface RangeRequest {
    /** No (usable) Range header: serve the whole file with 200. */
    data object Full : RangeRequest

    /** Serve [range] with 206. */
    data class Partial(val range: ByteRange) : RangeRequest

    /** Syntactically valid but outside the file: 416 with `Content-Range: bytes * /size`. */
    data object Unsatisfiable : RangeRequest
}

/**
 * Parses a `Range` header (RFC 9110 §14.2) for a file of [size] bytes. Only the first range of a multi-range request is honoured
 * (players never send more than one); unknown units and malformed values are ignored (= [RangeRequest.Full]) as the RFC allows.
 */
internal fun parseRange(header: String?, size: Long): RangeRequest {
    if (header.isNullOrBlank()) return RangeRequest.Full
    val value = header.trim()
    if (!value.regionMatches(0, "bytes=", 0, 6, ignoreCase = true)) return RangeRequest.Full
    val first = value.substring(6).substringBefore(',').trim()
    val dash = first.indexOf('-')
    if (dash < 0) return RangeRequest.Full
    val startText = first.substring(0, dash).trim()
    val endText = first.substring(dash + 1).trim()
    if (startText.isEmpty()) {
        // suffix range: the last N bytes
        val suffix = endText.toLongOrNull() ?: return RangeRequest.Full
        if (suffix <= 0 || size <= 0) return RangeRequest.Unsatisfiable
        return RangeRequest.Partial(ByteRange((size - suffix).coerceAtLeast(0), size - 1))
    }
    val start = startText.toLongOrNull()?.takeIf { it >= 0 } ?: return RangeRequest.Full
    if (start >= size) return RangeRequest.Unsatisfiable
    val end = if (endText.isEmpty()) size - 1 else endText.toLongOrNull() ?: return RangeRequest.Full
    if (end < start) return RangeRequest.Full
    return RangeRequest.Partial(ByteRange(start, minOf(end, size - 1)))
}

/** `Content-Type` by extension (same set as desktop tk-torrent + the other video containers of `isVideoPath`). */
internal fun contentTypeFor(name: String): String {
    val ext = name.substringAfterLast('.', "").lowercase(Locale.ROOT)
    return when (ext) {
        "mkv" -> "video/x-matroska"
        "mp4", "m4v" -> "video/mp4"
        "webm" -> "video/webm"
        "avi" -> "video/x-msvideo"
        "ts", "m2ts" -> "video/mp2t"
        "mov" -> "video/quicktime"
        "wmv" -> "video/x-ms-wmv"
        "flv" -> "video/x-flv"
        "ass", "ssa" -> "text/x-ssa"
        "srt" -> "application/x-subrip"
        else -> "application/octet-stream"
    }
}

/** Percent-encodes one path segment (RFC 3986 unreserved characters stay as they are). */
internal fun encodePathSegment(segment: String): String {
    val bytes = segment.toByteArray(Charsets.UTF_8)
    val out = StringBuilder(bytes.size * 2)
    for (b in bytes) {
        val c = b.toInt() and 0xff
        val unreserved = (c in 'a'.code..'z'.code) || (c in 'A'.code..'Z'.code) || (c in '0'.code..'9'.code) ||
            c == '-'.code || c == '.'.code || c == '_'.code || c == '~'.code
        if (unreserved) {
            out.append(c.toChar())
        } else {
            out.append('%').append(HEX[c shr 4]).append(HEX[c and 0x0f])
        }
    }
    return out.toString()
}

private val HEX = "0123456789ABCDEF".toCharArray()
