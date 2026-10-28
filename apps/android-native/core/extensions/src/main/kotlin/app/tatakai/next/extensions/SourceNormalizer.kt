// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Container
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.SubtitleTrack
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.put
import java.util.Locale

/** SourceResult -> SourceResultNormalized (routes.v1.json `types`) and the native mapping to `core:model` [StreamSource]. */
object SourceNormalizer {
    private val LANGUAGE_LABELS = mapOf(
        "ja" to "Japanese", "en" to "English", "es" to "Spanish", "pt" to "Portuguese", "fr" to "French",
        "de" to "German", "it" to "Italian", "ru" to "Russian", "ar" to "Arabic", "zh" to "Chinese", "ko" to "Korean",
    )

    fun normalize(raw: JsonObject, providerName: String, chunkPriority: Int?): JsonObject {
        val url = raw.str("url").orEmpty()
        val sourceType = classify(raw.str("sourceType"), url)
        val audio = raw.str("audioLanguage")
        val language = raw.str("language") ?: audio
        val priority = raw.int("providerPriority") ?: chunkPriority ?: 100
        val map = LinkedHashMap<String, JsonElement>(raw)
        map["providerName"] = JsonPrimitive(raw.str("providerName") ?: providerName)
        map["providerKey"] = JsonPrimitive((raw.str("providerKey") ?: providerName).lowercase(Locale.ROOT))
        map["providerPriority"] = JsonPrimitive(priority)
        map["server"] = JsonPrimitive(raw.str("server") ?: providerName)
        map["type"] = JsonPrimitive(sourceType)
        map["sourceType"] = JsonPrimitive(sourceType)
        map["fileFormat"] = JsonPrimitive(raw.str("fileFormat") ?: fileFormat(url))
        map["isM3U8"] = JsonPrimitive(sourceType == "hls")
        map["isEmbed"] = JsonPrimitive(sourceType == "embed")
        map["isTorrent"] = JsonPrimitive(sourceType == "torrent")
        if (audio != null) map["audioLanguage"] = JsonPrimitive(audio)
        if (language != null) {
            map["language"] = JsonPrimitive(language)
            map["languageLabel"] = JsonPrimitive(raw.str("languageLabel") ?: LANGUAGE_LABELS[language.lowercase(Locale.ROOT)] ?: language)
        }
        map["isDub"] = JsonPrimitive(raw.bool("isDub") ?: (audio != null && !audio.equals("ja", ignoreCase = true)))
        if ("headers" !in map) map["headers"] = JsonObject(emptyMap())
        if ("subtitles" !in map) map["subtitles"] = JsonArray(emptyList())
        return JsonObject(map)
    }

    /** "hls" | "mp4" | "torrent" | "embed". `custom`/unknown that is not a direct media URL counts as embed. */
    fun classify(declared: String?, url: String): String {
        val d = declared?.lowercase(Locale.ROOT)
        if (d == "hls" || d == "mp4" || d == "torrent" || d == "embed") return d
        val path = url.substringBefore('?').lowercase(Locale.ROOT)
        return when {
            url.startsWith("magnet:") || path.endsWith(".torrent") -> "torrent"
            path.endsWith(".m3u8") -> "hls"
            path.endsWith(".mp4") || path.endsWith(".m4v") || path.endsWith(".webm") || path.endsWith(".mkv") -> "mp4"
            else -> "embed"
        }
    }

    fun fileFormat(url: String): String {
        val path = url.substringBefore('?').lowercase(Locale.ROOT)
        val ext = path.substringAfterLast('.', "")
        return if (ext.length in 2..5) ext else "unknown"
    }

    /** Leading integer of a quality label ("1080p" -> 1080); 0 when none. */
    fun qualityRank(quality: String?): Int {
        return quality?.let { Regex("""\d+""").find(it)?.value?.toIntOrNull() } ?: 0
    }

    /** providerPriority ascending, then quality descending; stable for ties. */
    fun sort(sources: List<JsonObject>): List<JsonObject> =
        sources.sortedWith(
            compareBy<JsonObject> { it.int("providerPriority") ?: 100 }.thenByDescending { qualityRank(it.str("quality")) },
        )

    fun summarize(sources: List<JsonObject>): Triple<JsonObject, JsonObject, Int> {
        val byLang = LinkedHashMap<String, Int>()
        val byType = LinkedHashMap<String, Int>()
        for (s in sources) {
            byLang.merge(s.str("language") ?: "unknown", 1, Int::plus)
            byType.merge(s.str("type") ?: "unknown", 1, Int::plus)
        }
        return Triple(
            JsonObject(byLang.mapValues { JsonPrimitive(it.value) }),
            JsonObject(byType.mapValues { JsonPrimitive(it.value) }),
            sources.size,
        )
    }

    /**
     * routes.v1.json `nativeMapping`: only hls/mp4 (and dash when the URL ends in `.mpd`) are playable in M0. Embeds and torrents are
     * skipped. The input order (providerPriority asc, quality desc) is preserved.
     */
    fun toStreamSources(response: JsonObject, extensionId: String): List<StreamSource> {
        val sources = (response["sources"] as? JsonArray).orEmpty()
        return sources.mapNotNull { el ->
            val s = el as? JsonObject ?: return@mapNotNull null
            val url = s.str("url") ?: return@mapNotNull null
            if (NetworkPolicy.parseHttpUri(url) == null) return@mapNotNull null
            val container = when {
                url.substringBefore('?').lowercase(Locale.ROOT).endsWith(".mpd") -> Container.DASH
                s.str("type") == "hls" -> Container.HLS
                s.str("type") == "mp4" -> Container.MP4
                else -> return@mapNotNull null
            }
            val headers = (s["headers"] as? JsonObject)?.mapNotNull { (k, v) ->
                (v as? JsonPrimitive)?.contentOrNull?.let { k to it }
            }?.toMap().orEmpty()
            val subs = (s["subtitles"] as? JsonArray).orEmpty().mapNotNull { sub ->
                val so = sub as? JsonObject ?: return@mapNotNull null
                val subUrl = so.str("url") ?: return@mapNotNull null
                SubtitleTrack(
                    url = subUrl,
                    lang = so.str("language") ?: so.str("lang") ?: "und",
                    label = so.str("label") ?: so.str("language") ?: "",
                    format = subtitleFormat(subUrl),
                )
            }
            StreamSource(
                url = url,
                headers = headers,
                container = container,
                quality = s.str("quality"),
                provider = s.str("providerName") ?: extensionId,
                extensionId = extensionId,
                subtitles = subs,
            )
        }
    }

    fun subtitleFormat(url: String): String {
        val ext = url.substringBefore('?').substringAfterLast('.', "").lowercase(Locale.ROOT)
        return if (ext == "srt" || ext == "ass" || ext == "vtt") ext else "vtt"
    }

    fun envelope(namespace: String, extra: JsonObject, now: String): JsonObject = buildJsonObject {
        put("namespace", namespace)
        extra.forEach { (k, v) -> put(k, v) }
        put("fetchedAt", now)
    }
}

internal fun JsonObject.str(key: String): String? = (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.contentOrNull

internal fun JsonObject.int(key: String): Int? = (this[key] as? JsonPrimitive)?.doubleOrNull?.toInt()

internal fun JsonObject.bool(key: String): Boolean? = (this[key] as? JsonPrimitive)?.let { runCatching { it.boolean }.getOrNull() }
