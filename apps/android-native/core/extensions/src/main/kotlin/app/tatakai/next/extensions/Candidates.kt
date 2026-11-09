// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Container
import app.tatakai.next.model.EMBED_CONTAINER_LABEL
import app.tatakai.next.model.INITIAL_RELIABILITY
import app.tatakai.next.model.Quality
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SubtitleTrack
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.WebInfo
import app.tatakai.next.release.langCodes
import app.tatakai.next.release.parseRelease
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull
import kotlin.math.roundToLong

// Port of apps/desktop-native/crates/tk-ext/src/candidates.rs (docs/redesign/10 §6, 12 §3.6 / §4 e): route bodies -> SourceCandidate.
// Every selector field is optional in the extension output; nothing here throws; an entry that cannot be listed at all (torrent
// without a hash, a web entry without URL) is skipped. parity wave B (16 §3.3, worker t): an `embed` page is listed as a web candidate
// with `WebInfo.embed = true` and `container = EMBED_CONTAINER_LABEL` (played through the EmbedSniffer). Torrent entries also accept Toko's field names (torrentTitle, magnetLink, releaseGroup, fileSize,
// isBest): the release name is parsed with :core:release parseRelease into SourceCandidate.tags.

object Candidates {
    /**
     * Routes `resolveCandidates` asks one enabled, runnable extension, in order, each only if `apiServer.routes` allows it: `torrent` for
     * `torrent` extensions; `stream` for `custom` / `onlinestream` ones, plus `torrent` when their manifest lists it explicitly in
     * `apiServer.routes` (the default list does not count) or their bundle exposes `torrentAll` ([exposesTorrentAll]).
     */
    fun candidateRoutes(m: ExtensionManifest, exposesTorrentAll: Boolean): List<String> {
        val gated = m.enabledAggregateRoutes
        if (m.type == ExtensionType.TORRENT) return if ("torrent" in gated) listOf("torrent") else emptyList()
        val api = m.apiServer
        val listsTorrent = api != null && api.routesExplicit && "torrent" in api.routes
        val out = ArrayList<String>(2)
        if ("stream" in gated) out += "stream"
        if ((listsTorrent || exposesTorrentAll) && "torrent" in gated) out += "torrent"
        return out
    }

    /** Whether the bundle text mentions the identifier `torrentAll` (word-bounded; `$` counts as an identifier character). */
    fun exposesTorrentAll(bundle: String): Boolean {
        val kw = "torrentAll"
        fun ident(c: Char) = c.isAsciiLetterOrDigit() || c == '_' || c == '$'
        var from = 0
        while (true) {
            val i = bundle.indexOf(kw, from)
            if (i < 0) return false
            val end = i + kw.length
            if ((i == 0 || !ident(bundle[i - 1])) && (end >= bundle.length || !ident(bundle[end]))) return true
            from = end
        }
    }

    /**
     * Map one route body (`{ sources: SourceResultNormalized[] }`) of extension [extId] (display name [extName]). Order is kept; labels
     * repeated inside one body get the quality (or `#n`) appended so every candidate has its own `sourceKey`.
     */
    fun mapCandidates(extId: String, extName: String, body: JsonObject): List<SourceCandidate> {
        val list = body["sources"] as? JsonArray ?: return emptyList()
        val out = ArrayList<SourceCandidate>()
        for (el in list) {
            val s = el as? JsonObject ?: continue
            var c = mapOne(extId, extName, s) ?: continue
            if (taken(out, c.kind, c.label)) {
                val q = c.quality.label()
                val withQ = "${c.label} $q"
                c = if (q.isNotEmpty() && !taken(out, c.kind, withQ)) {
                    c.copy(label = withQ)
                } else {
                    var n = 2
                    while (taken(out, c.kind, "${c.label} #$n")) n++
                    c.copy(label = "${c.label} #$n")
                }
            }
            out += c
        }
        return out
    }

    private fun taken(out: List<SourceCandidate>, kind: SourceKind, label: String) = out.any { it.kind == kind && it.label == label }

    private fun mapOne(extId: String, extName: String, s: JsonObject): SourceCandidate? {
        val url = s.string("url") ?: ""
        val ty = s.string("type") ?: s.string("sourceType") ?: ""
        val provider = text(s["providerName"]) ?: extId
        val quality = when (val q = s["quality"]) {
            is JsonPrimitive -> if (q.isString || q.doubleOrNull != null) Quality.parse(q.content) else Quality.UNKNOWN
            else -> Quality.UNKNOWN
        }
        val subtitles = subtitlesOf(s)
        val subs = stringListOf(s["subs"])?.let { langCodes(it) } ?: langCodes(subtitles.map { it.lang })
        val dubs = stringListOf(s["dubs"])?.let { langCodes(it) } ?: langCodes(listOfNotNull(s.string("audioLanguage")))
        val note = text(s["note"])
        if (ty == "torrent" || (s["isTorrent"] as? JsonPrimitive)?.booleanOrNull == true) {
            fun isMagnet(u: String) = u.asciiLower().startsWith("magnet:")
            val magnet = url.takeIf(::isMagnet)
                ?: text(s["magnetLink"])?.takeIf(::isMagnet)
                ?: text(s["magnet"])?.takeIf(::isMagnet)
            val infoHash = (text(s["infoHash"]) ?: text(s["hash"]) ?: magnet?.let(::btih))?.let(::hexInfoHash) ?: return null
            val releaseName = text(s["torrentTitle"]) ?: text(s["title"]) ?: text(s["name"]) ?: text(s["releaseName"])
                ?: text(s["fileName"]) ?: magnet?.let(::magnetName)
            var tags = releaseName?.let(::parseRelease) ?: ReleaseTags()
            if (tags.resolution == Quality.UNKNOWN) tags = tags.copy(resolution = quality)
            if (s.bool("dualAudio") == true) tags = tags.copy(dualAudio = true)
            if (s.bool("isBatch") == true || s.bool("batch") == true) tags = tags.copy(batch = true)
            val explicitBest = listOf("seadexBest", "isBest", "best").firstNotNullOfOrNull { s.bool(it) }
            val fromSeadex = listOf("providerName", "source", "providerKey").any { s.string(it)?.equals("seadex", ignoreCase = true) == true }
            tags = tags.copy(seadexBest = explicitBest ?: fromSeadex)
            val finalQuality = if (quality == Quality.UNKNOWN) tags.resolution else quality
            val label = text(s["group"]) ?: text(s["releaseGroup"]) ?: tags.group ?: text(s["server"]) ?: provider
            val torrent = TorrentInfo(
                infoHash = infoHash,
                sizeBytes = sizeOf(s["sizeBytes"]) ?: sizeOf(s["fileSize"]) ?: sizeOf(s["size"]),
                seeders = count(s["seeders"]),
                leechers = count(s["leechers"]),
                magnet = magnet,
            )
            return SourceCandidate(
                extId = extId,
                extName = extName,
                kind = SourceKind.TORRENT,
                label = label,
                quality = finalQuality,
                container = text(s["codec"]) ?: text(s["container"]),
                subs = subs,
                dubs = dubs,
                reliability = INITIAL_RELIABILITY,
                note = note,
                web = null,
                torrent = torrent,
                tags = tags,
            )
        }
        if (url.isEmpty()) return null
        val embed = isEmbedEntry(s, url)
        val container = if (embed) null else (webContainer(s, url) ?: return null)
        val label = text(s["server"]) ?: provider
        return SourceCandidate(
            extId = extId,
            extName = extName,
            kind = SourceKind.WEB,
            label = label,
            quality = quality,
            container = container?.let(::containerLabel) ?: EMBED_CONTAINER_LABEL,
            subs = subs,
            dubs = dubs,
            reliability = INITIAL_RELIABILITY,
            note = note,
            web = WebInfo(
                url = url,
                headers = headersOf(s),
                latencyMs = (s["latencyMs"] as? JsonPrimitive)?.takeUnless { it.isString }?.longOrNull?.takeIf { it >= 0 }
                    ?.coerceAtMost(Int.MAX_VALUE.toLong())?.toInt(),
                subtitles = subtitles,
                embed = embed,
            ),
            torrent = null,
            tags = ReleaseTags(),
        )
    }

    /** `type` / `sourceType` == `embed` (or `isEmbed: true`) with an http(s) page URL: needs the sniffer before it can play. */
    internal fun isEmbedEntry(s: JsonObject, url: String): Boolean {
        val declared = (s.string("type") ?: s.string("sourceType") ?: "").asciiLower()
        val flagged = declared == "embed" || (declared.isEmpty() && s.bool("isEmbed") == true)
        val lower = url.asciiLower()
        return flagged && (lower.startsWith("https://") || lower.startsWith("http://"))
    }

    // ---- field helpers (desktop candidates.rs / host.rs) -----------------------------------------------------------------------

    private fun JsonObject.string(key: String): String? = (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content

    private fun JsonObject.bool(key: String): Boolean? = (this[key] as? JsonPrimitive)?.takeUnless { it.isString }?.booleanOrNull

    /** Non-empty trimmed string (only JSON strings). */
    private fun text(v: JsonElement?): String? = (v as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() }

    /** `["es","en"]`, or a single `"es"` / `"es, en"`; null when the field is absent or of another type. */
    private fun stringListOf(v: JsonElement?): List<String>? = when {
        v is JsonArray -> v.mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content }
        v is JsonPrimitive && v.isString -> v.content.split(',', ' ').filter { it.isNotEmpty() }
        else -> null
    }

    internal fun count(v: JsonElement?): Int {
        val p = v as? JsonPrimitive ?: return 0
        if (p.isString) return p.content.trim().toLongOrNull()?.takeIf { it >= 0 }?.coerceAtMost(Int.MAX_VALUE.toLong())?.toInt() ?: 0
        p.longOrNull?.let { return if (it < 0) 0 else it.coerceAtMost(Int.MAX_VALUE.toLong()).toInt() }
        val d = p.doubleOrNull ?: return 0
        return if (d > 0.0) d.coerceAtMost(Int.MAX_VALUE.toDouble()).toInt() else 0
    }

    /** Bytes from a number or a display string (`"1.38 GB"`, `"700 MiB"`); decimal units are 1000-based. */
    internal fun sizeOf(v: JsonElement?): Long? {
        val p = v as? JsonPrimitive ?: return null
        if (!p.isString) {
            p.longOrNull?.let { return if (it >= 0) it else null }
            return p.doubleOrNull?.takeIf { it >= 0.0 }?.toLong()
        }
        val s = p.content.trim()
        val split = s.indexOfFirst { !(it in '0'..'9' || it == '.' || it == ',') }.let { if (it < 0) s.length else it }
        val num = s.substring(0, split).replace(',', '.').toDoubleOrNull() ?: return null
        val mult = when (s.substring(split).trim().asciiLower()) {
            "", "b" -> 1.0
            "kb" -> 1e3
            "mb" -> 1e6
            "gb" -> 1e9
            "tb" -> 1e12
            "kib" -> 1024.0
            "mib" -> 1024.0 * 1024.0
            "gib" -> 1024.0 * 1024.0 * 1024.0
            "tib" -> 1024.0 * 1024.0 * 1024.0 * 1024.0
            else -> return null
        }
        return (num * mult).roundToLong()
    }

    /** `dn=<name>` of a magnet link, percent-decoded (`+` is a space). */
    internal fun magnetName(magnet: String): String? {
        val q = magnet.indexOf('?')
        if (q < 0) return null
        val raw = magnet.substring(q + 1).split('&').firstNotNullOfOrNull { kv -> kv.takeIf { it.startsWith("dn=") }?.substring(3) }
            ?: return null
        val bytes = raw.toByteArray(Charsets.UTF_8)
        val out = java.io.ByteArrayOutputStream(bytes.size)
        var i = 0
        while (i < bytes.size) {
            val b = bytes[i]
            when {
                b == '+'.code.toByte() -> out.write(' '.code)
                b == '%'.code.toByte() && i + 2 < bytes.size -> {
                    val hi = Character.digit(bytes[i + 1].toInt().toChar(), 16)
                    val lo = Character.digit(bytes[i + 2].toInt().toChar(), 16)
                    if (hi >= 0 && lo >= 0 && bytes[i + 1] >= 0 && bytes[i + 2] >= 0) {
                        out.write(hi * 16 + lo)
                        i += 2
                    } else {
                        out.write('%'.code)
                    }
                }
                else -> out.write(b.toInt())
            }
            i++
        }
        return out.toByteArray().toString(Charsets.UTF_8).trim().takeIf { it.isNotEmpty() }
    }

    /** A v1 info-hash as 40 lowercase hex chars: hex is kept, base32 (32 chars, RFC 4648) is converted; anything else is null. */
    fun hexInfoHash(h: String): String? {
        val t = h.trim()
        if (t.length == 40 && t.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }) return t.asciiLower()
        if (t.length != 32) return null
        var bits = 0L
        var nbits = 0
        val out = StringBuilder(40)
        for (ch in t) {
            val c = if (ch in 'a'..'z') ch - 32 else ch
            val v = when (c) {
                in 'A'..'Z' -> c - 'A'
                in '2'..'7' -> c - '2' + 26
                else -> return null
            }
            bits = (bits shl 5) or v.toLong()
            nbits += 5
            while (nbits >= 8) {
                nbits -= 8
                out.append(String.format(java.util.Locale.ROOT, "%02x", (bits shr nbits) and 0xff))
            }
            bits = bits and ((1L shl nbits) - 1)
        }
        return out.toString().takeIf { it.length == 40 }
    }

    /** `xt=urn:btih:<hash>` of a magnet link. */
    internal fun btih(magnet: String): String? {
        val key = "xt=urn:btih:"
        val start = magnet.asciiLower().indexOf(key).takeIf { it >= 0 }?.plus(key.length) ?: return null
        val rest = magnet.substring(start)
        val end = rest.indexOf('&').let { if (it < 0) rest.length else it }
        return rest.substring(0, end).takeIf { it.isNotEmpty() }
    }

    /** Playable container of a normalized source (`type` / `sourceType`); null for embed / custom / torrent. */
    internal fun webContainer(s: JsonObject, url: String): Container? =
        when (s.string("type") ?: s.string("sourceType") ?: "") {
            "hls" -> Container.HLS
            "mp4" -> Container.MP4
            "dash" -> if (url.split('?', '#').first().endsWith(".mpd")) Container.DASH else null
            else -> null
        }

    /** Display label of a web container (desktop `container_label`). */
    internal fun containerLabel(c: Container): String = when (c) {
        Container.HLS -> "HLS"
        Container.MP4 -> "MP4"
        Container.DASH -> "DASH"
        Container.PROGRESSIVE -> "MP4"
    }

    internal fun headersOf(s: JsonObject): Map<String, String> =
        (s["headers"] as? JsonObject)?.mapNotNull { (k, v) -> (v as? JsonPrimitive)?.takeIf { it.isString }?.content?.let { k to it } }
            ?.toMap().orEmpty()

    internal fun subtitlesOf(s: JsonObject): List<SubtitleTrack> =
        (s["subtitles"] as? JsonArray).orEmpty().mapNotNull { t ->
            val o = t as? JsonObject ?: return@mapNotNull null
            val u = o.string("url")?.takeIf { it.isNotEmpty() } ?: return@mapNotNull null
            SubtitleTrack(
                url = u,
                lang = o.string("language") ?: o.string("lang") ?: "und",
                label = o.string("label") ?: "",
                format = subtitleFormat(u),
            )
        }

    private fun subtitleFormat(url: String): String {
        val path = url.split('?', '#').first().asciiLower()
        return when {
            path.endsWith(".srt") -> "srt"
            path.endsWith(".ass") || path.endsWith(".ssa") -> "ass"
            else -> "vtt"
        }
    }

    private fun Char.isAsciiLetterOrDigit() = this in 'a'..'z' || this in 'A'..'Z' || this in '0'..'9'
}
