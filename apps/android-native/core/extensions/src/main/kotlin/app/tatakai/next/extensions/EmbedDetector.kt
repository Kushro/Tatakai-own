// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Container
import java.net.URI
import java.util.Locale

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.3, worker t; checklist sources.embed-sniffer,
 * sources.embed-sniffer.adblock): the pure decisions of the embed sniffer (`WebViewEmbedSniffer`), kept out of the WebView so they are
 * unit-tested: which request is the media, which host is an ad / popunder host, which navigation leaves the embed. Port of desktop
 * `sniffer::{is_media_url, is_blocked_host}` (tk-app/src/live/sniffer.rs).
 */
object EmbedDetector {
    /** HLS / MP4 by URL path (query and fragment ignored) or by content type; DASH and everything else is null (not playable here). */
    fun mediaContainer(url: String, contentType: String? = null): Container? {
        val type = contentType?.substringBefore(';')?.trim()?.lowercase(Locale.ROOT)
        when (type) {
            "application/vnd.apple.mpegurl", "application/x-mpegurl", "audio/mpegurl", "audio/x-mpegurl" -> return Container.HLS
            "video/mp4" -> return Container.MP4
        }
        val lower = url.lowercase(Locale.ROOT)
        if (!lower.startsWith("http://") && !lower.startsWith("https://")) return null
        val path = lower.substringBefore('#').substringBefore('?')
        return when {
            path.endsWith(".m3u8") -> Container.HLS
            path.endsWith(".mp4") || path.endsWith(".m4v") -> Container.MP4
            // CDNs that hide the extension in the query (`file.php?url=...m3u8`, `/hls/...?type=m3u8`).
            lower.substringAfter('?', "").let { q -> q.contains(".m3u8") || q.contains("type=m3u8") || q.contains("format=m3u8") } -> Container.HLS
            else -> null
        }
    }

    /** True for subtitle / thumbnail / segment requests that look like media but are not the stream (`.ts` segments, `.vtt`). */
    fun isIgnorable(url: String): Boolean {
        val path = url.lowercase(Locale.ROOT).substringBefore('#').substringBefore('?')
        return IGNORED_SUFFIXES.any { path.endsWith(it) }
    }

    /** Lower-case host of [url], or null when it does not parse. */
    fun hostOf(url: String): String? = runCatching { URI(url).host?.lowercase(Locale.ROOT)?.trimEnd('.') }.getOrNull()?.takeIf { it.isNotEmpty() }

    /** [host] or one of its parent domains is in [blocked] (`ads.example.com` matches `example.com`). */
    fun isBlockedHost(host: String, blocked: Set<String>): Boolean {
        var h = host.lowercase(Locale.ROOT).trimEnd('.')
        while (true) {
            if (h in blocked) return true
            val dot = h.indexOf('.')
            if (dot < 0) return false
            h = h.substring(dot + 1)
            if (!h.contains('.')) return h in blocked
        }
    }

    /** One host per line; `#` comments, blank lines and hosts-file prefixes (`0.0.0.0 x`, `127.0.0.1 x`) are accepted. */
    fun parseHostList(text: String): Set<String> =
        text.lineSequence()
            .map { it.substringBefore('#').trim() }
            .filter { it.isNotEmpty() }
            .map { line -> line.split(Regex("\\s+")).last().lowercase(Locale.ROOT).trimEnd('.') }
            .filter { it.contains('.') && HOST.matches(it) }
            .toSet()

    /**
     * A top-level navigation of the hidden page is allowed only inside the embed's own site (same registrable-ish domain: the last two
     * labels) or to one of [extraAllowed]; everything else (redirect to an ad, a store, a popunder) is cancelled.
     */
    fun navigationAllowed(embedUrl: String, target: String, extraAllowed: Set<String> = emptySet()): Boolean {
        val target0 = target.lowercase(Locale.ROOT)
        if (target0.startsWith("about:") || target0.startsWith("blob:") || target0.startsWith("data:")) return true
        if (!target0.startsWith("http://") && !target0.startsWith("https://")) return false
        val from = hostOf(embedUrl) ?: return false
        val to = hostOf(target) ?: return false
        return siteOf(from) == siteOf(to) || extraAllowed.any { to == it || to.endsWith(".$it") }
    }

    /** `player.cdn.example.co` -> `example.co` (last two labels; good enough to keep a player inside its site). */
    fun siteOf(host: String): String = host.split('.').takeLast(2).joinToString(".")

    /** Only these request headers are replayed to the player (the rest are WebView internals). */
    fun replayHeaders(requestHeaders: Map<String, String>, cookie: String?, userAgent: String?): Map<String, String> {
        val out = LinkedHashMap<String, String>()
        for ((k, v) in requestHeaders) {
            val key = k.lowercase(Locale.ROOT)
            if (key in REPLAYED && v.isNotBlank()) out[CANONICAL.getValue(key)] = v
        }
        if (!cookie.isNullOrBlank()) out["Cookie"] = cookie
        if (!userAgent.isNullOrBlank() && "User-Agent" !in out) out["User-Agent"] = userAgent
        return out
    }

    private val CANONICAL = mapOf("referer" to "Referer", "origin" to "Origin", "user-agent" to "User-Agent", "cookie" to "Cookie", "accept-language" to "Accept-Language")
    private val REPLAYED = CANONICAL.keys
    private val IGNORED_SUFFIXES = listOf(".ts", ".m4s", ".vtt", ".srt", ".jpg", ".jpeg", ".png", ".webp", ".gif", ".css", ".js", ".woff", ".woff2")
    private val HOST = Regex("^[a-z0-9.-]{3,253}$")
}
