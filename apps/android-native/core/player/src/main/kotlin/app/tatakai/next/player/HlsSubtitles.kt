// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.IOException
import kotlin.coroutines.coroutineContext

// 0.4.0 (sub-translation worker): the full cue list of an HLS subtitle rendition (`#EXT-X-MEDIA:TYPE=SUBTITLES`, segmented WebVTT),
// the "embedded" subtitles of web streams. Both engines play them as in-stream tracks, so before this only live translation worked.
// The rendition's segments are small text files: they are all fetched (capped) and joined, honouring `X-TIMESTAMP-MAP`.

/** One `#EXT-X-MEDIA:TYPE=SUBTITLES` entry; [ordinal] = order among the master's subtitle renditions. */
data class HlsSubtitleRendition(val ordinal: Int, val name: String?, val language: String?, val uri: String)

object HlsSubtitles {
    private val ATTR = Regex("""([A-Z0-9-]+)=("[^"]*"|[^,]*)""")
    private val TIMESTAMP_MAP = Regex("""X-TIMESTAMP-MAP=([^\n]*)""")
    private const val MAX_SEGMENTS = 1_500
    private const val MAX_BYTES = 6L * 1024 * 1024

    fun isHls(url: String): Boolean = url.substringBefore('?').substringBefore('#').lowercase().endsWith(".m3u8")

    /** Subtitle renditions of a master playlist (URIs resolved against [baseUrl]). */
    fun renditions(master: String, baseUrl: String): List<HlsSubtitleRendition> {
        val out = ArrayList<HlsSubtitleRendition>()
        for (raw in master.lineSequence()) {
            val line = raw.trim()
            if (!line.startsWith("#EXT-X-MEDIA:")) continue
            val attrs = ATTR.findAll(line.substringAfter(':')).associate { it.groupValues[1] to it.groupValues[2].trim('"') }
            if (attrs["TYPE"] != "SUBTITLES") continue
            val uri = attrs["URI"]?.takeIf { it.isNotBlank() } ?: continue
            out += HlsSubtitleRendition(out.size, attrs["NAME"], attrs["LANGUAGE"], resolve(baseUrl, uri))
        }
        return out
    }

    /** Segment URIs of a media playlist. */
    fun segments(media: String, baseUrl: String): List<String> =
        media.lineSequence().map { it.trim() }.filter { it.isNotEmpty() && !it.startsWith("#") }.map { resolve(baseUrl, it) }.toList()

    /** The rendition the player selected (same rules as [EmbeddedTrackMatcher]). */
    fun match(renditions: List<HlsSubtitleRendition>, want: EmbeddedTrackRef): HlsSubtitleRendition? {
        val asTracks = renditions.map { EmbeddedTrack(it.ordinal, it.ordinal, androidx.media3.common.MimeTypes.TEXT_VTT, it.language, it.name) }
        // The engines list renditions in master order but may also number other in-stream tracks: title match wins when present.
        renditions.firstOrNull { r -> !want.title.isNullOrBlank() && r.name == want.title }?.let { return it }
        val hit = EmbeddedTrackMatcher.match(asTracks, want) ?: return null
        return renditions.getOrNull(hit.ordinal)
    }

    /**
     * One WebVTT segment -> cues on the stream timeline. `X-TIMESTAMP-MAP=MPEGTS:<pts>,LOCAL:<time>` maps a cue time to the 90 kHz
     * clock; players put the first segment's mapped PTS at the stream start, so the offset is relative to [baseMpegTs] (the first
     * segment's MPEGTS) and LOCAL is subtracted.
     */
    fun parseSegment(vtt: String, baseMpegTs: Long?): Pair<List<TimedCue>, Long?> {
        var mpegTs: Long? = null
        var local = 0L
        TIMESTAMP_MAP.find(vtt)?.groupValues?.get(1)?.split(',')?.forEach { part ->
            val (k, v) = part.split(':', limit = 2).let { (it.getOrNull(0)?.trim() ?: "") to (it.getOrNull(1)?.trim() ?: "") }
            when (k) {
                "MPEGTS" -> mpegTs = v.toLongOrNull()
                "LOCAL" -> local = SubtitleParser.timestamp(v) ?: 0L
            }
        }
        val base = baseMpegTs ?: mpegTs
        val shift = (mpegTs?.let { ts -> base?.let { (ts - it) / 90 } } ?: 0L) - local
        val cues = SubtitleParser.parse(vtt, "vtt").map { TimedCue(it.startMs + shift, it.endMs + shift, it.text) }
        return cues to base
    }

    /** Downloads the [want] rendition of the master at [masterUrl]; null when it has no matching subtitle rendition. */
    suspend fun fetch(client: OkHttpClient, masterUrl: String, headers: Map<String, String>, want: EmbeddedTrackRef): List<TimedCue>? =
        withContext(Dispatchers.IO) {
            val master = get(client, masterUrl, headers)
            val rendition = match(renditions(master, masterUrl), want) ?: return@withContext null
            val media = get(client, rendition.uri, headers)
            val segs = segments(media, rendition.uri).take(MAX_SEGMENTS)
            var base: Long? = null
            var bytes = 0L
            val seen = HashSet<Triple<Long, Long, String>>()
            val out = ArrayList<TimedCue>()
            for (seg in segs) {
                coroutineContext.ensureActive()
                val text = runCatching { get(client, seg, headers) }.getOrNull() ?: continue
                bytes += text.length
                if (bytes > MAX_BYTES) break
                val (cues, b) = parseSegment(text, base)
                base = b
                // A cue spanning a segment boundary is repeated in both segments.
                cues.forEach { c -> if (seen.add(Triple(c.startMs, c.endMs, c.text))) out += c }
            }
            out.sortedBy { it.startMs }
        }

    private fun get(client: OkHttpClient, url: String, headers: Map<String, String>): String {
        val req = Request.Builder().url(url).apply { headers.forEach { (k, v) -> header(k, v) } }.build()
        return client.newCall(req).execute().use { res ->
            if (!res.isSuccessful) throw IOException("HTTP ${res.code}")
            res.body.string()
        }
    }

    private fun resolve(base: String, ref: String): String = base.toHttpUrlOrNull()?.resolve(ref)?.toString() ?: ref
}
