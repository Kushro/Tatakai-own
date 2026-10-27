// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.put
import java.time.Instant
import java.util.concurrent.ConcurrentHashMap

/** An extension as the host sees it: package contents plus the user's enable switch. */
class LoadedExtension(
    val manifest: ExtensionManifest,
    val bundle: String,
    val compat: CompatLevel,
    val enabled: Boolean,
)

/**
 * Native replacement of the legacy `extension-api-host` (contracts/extension-host/routes.v1.json): the same logical routes as in-process
 * calls, `call(extensionId, route, params)`. One [JsSession] (= one QuickJS runtime) per extension, calls serialized per extension; the
 * runtime is discarded after a timeout / OOM and re-created on the next call.
 */
class ExtensionHost(
    private val engine: JsEngine,
    private val hostFor: (ExtensionManifest) -> JsHost,
    private val clock: () -> Instant = { Instant.now() },
) {
    private class Slot(var ext: LoadedExtension) {
        val mutex = Mutex()
        var session: JsSession? = null
    }

    private class CacheEntry(val at: Instant, val value: JsonObject)

    private val slots = ConcurrentHashMap<String, Slot>()
    private val cache = ConcurrentHashMap<String, CacheEntry>()

    fun register(extension: LoadedExtension) {
        val id = extension.manifest.id
        val existing = slots[id]
        if (existing == null) {
            slots[id] = Slot(extension)
        } else {
            existing.ext = extension
            existing.session?.runCatching { close() }
            existing.session = null
        }
        cache.keys.removeAll { it.startsWith("$id|") }
    }

    fun unregister(id: String) {
        slots.remove(id)?.session?.runCatching { close() }
        cache.keys.removeAll { it.startsWith("$id|") }
    }

    fun namespaces(): List<JsonObject> = slots.values.map { namespaceInfo(it.ext.manifest) }

    fun namespaceInfo(m: ExtensionManifest): JsonObject = buildJsonObject {
        put("namespace", m.namespace)
        put("extensionId", m.id)
        put("contract", m.apiServer?.contract?.let { JsonPrimitive(it) } ?: JsonNull)
        put("routes", JsonArray(m.enabledAggregateRoutes.map { JsonPrimitive(it) }))
        put("providerRoutes", JsonArray(listOf(JsonPrimitive("providers"), JsonPrimitive("providers/:name"))))
        put("capabilities", JsonArray(m.capabilities.map { JsonPrimitive(it) }))
        put(
            "customSources",
            JsonArray(
                m.customSources.map {
                    buildJsonObject {
                        put("id", it.id); put("kind", it.kind); put("name", it.name)
                        it.icon?.let { v -> put("icon", v) }
                        it.description?.let { v -> put("description", v) }
                    }
                },
            ),
        )
        put(
            "customRoutes",
            JsonArray(if (m.customSources.isEmpty()) emptyList() else CUSTOM_ROUTES.map { JsonPrimitive(it) }),
        )
    }

    /**
     * @param params the legacy query-string keys as a JSON object (`titles[]` may be an array under `titles` or `titles[]`).
     * @return the JSON body the legacy host returned. @throws ExtError
     */
    suspend fun call(extensionId: String, route: String, params: JsonObject = JsonObject(emptyMap())): JsonObject {
        val slot = slots[extensionId] ?: throw ExtError.NotFound("Unknown extension: $extensionId")
        val ext = slot.ext
        if (ext.compat == CompatLevel.UNSUPPORTED) {
            throw ExtError.Unsupported("Extension '${ext.manifest.id}' is not supported by the native runtime")
        }
        if (!ext.enabled) throw ExtError.Unsupported("Extension '${ext.manifest.id}' is disabled")

        val path = route.trim('/').removePrefix("api/v3/${ext.manifest.namespace}/").trim('/')
        val m = ext.manifest
        return when {
            path == "health" -> buildJsonObject {
                put("ok", true); put("namespace", m.namespace); put("extensionId", m.id)
            }
            path == "sources" || path == "stream" || path == "torrent" -> aggregate(slot, path, params)
            path == "providers" -> providers(slot)
            path.startsWith("providers/") -> provider(slot, path.removePrefix("providers/"), params)
            else -> generic(slot, path, params)
        }
    }

    // ---- aggregate routes -------------------------------------------------------------------------------------------------------

    private suspend fun aggregate(slot: Slot, route: String, params: JsonObject): JsonObject {
        val m = slot.ext.manifest
        if (route !in m.enabledAggregateRoutes) throw ExtError.NotFound("Route '$route' is not enabled for ${m.id}")
        val bypass = params.flag("refresh") || params.flag("nocache")
        val key = "${m.id}|$route|${canonical(params)}"
        val ttl = if (route == "torrent") TORRENT_TTL_S else AGGREGATE_TTL_S
        if (!bypass) {
            cache[key]?.let { hit ->
                if (clock().epochSecond - hit.at.epochSecond < ttl) {
                    return JsonObject(hit.value + ("cached" to JsonPrimitive(true)))
                }
            }
        }
        val opts = sourceOptions(params)
        val (primary, fallback) = when (route) {
            "stream" -> "streamAll" to "single"
            "torrent" -> "torrentAll" to "batch"
            else -> "sourcesAll" to "batch"
        }
        val chunks = ArrayList<JsonElement>()
        var returned: JsonElement = JsonNull
        withSession(slot) { session ->
            val method = when {
                session.hasMethod(primary) -> primary
                session.hasMethod(fallback) -> fallback
                else -> throw ExtError.NotImplemented("Bundle does not implement $primary or $fallback")
            }
            val r = session.invoke(method, listOf(opts), collectChunks = method == primary)
            chunks += r.chunks
            returned = r.result
        }
        val body = buildAggregate(m, route, chunks, returned)
        cache[key] = CacheEntry(clock(), body)
        return body
    }

    private fun buildAggregate(m: ExtensionManifest, route: String, chunks: List<JsonElement>, returned: JsonElement): JsonObject {
        val sources = ArrayList<JsonObject>()
        val statuses = ArrayList<JsonElement>()

        fun absorbChunk(chunk: JsonElement) {
            val o = chunk as? JsonObject ?: return
            val provider = o.str("provider") ?: m.id
            val results = (o["results"] as? JsonArray).orEmpty()
            results.forEach { r -> (r as? JsonObject)?.let { sources += SourceNormalizer.normalize(it, provider, null) } }
            (o["diagnostic"] as? JsonObject)?.let { statuses += it }
        }

        when {
            chunks.isNotEmpty() -> chunks.forEach(::absorbChunk)
            returned is JsonArray -> returned.forEach { r ->
                (r as? JsonObject)?.let { sources += SourceNormalizer.normalize(it, m.id, null) }
            }
            returned is JsonObject && returned["sources"] is JsonArray -> {
                (returned["sources"] as JsonArray).forEach { r ->
                    (r as? JsonObject)?.let { sources += SourceNormalizer.normalize(it, it.str("providerName") ?: m.id, null) }
                }
                (returned["providerStatus"] as? JsonArray)?.let { statuses.addAll(it) }
            }
            returned is JsonObject && returned["results"] is JsonArray -> absorbChunk(returned)
        }
        val filtered = if (route == "torrent") sources.filter { it.str("type") == "torrent" } else sources
        val sorted = SourceNormalizer.sort(filtered)
        val (byLanguage, byType, count) = SourceNormalizer.summarize(sorted)
        return buildJsonObject {
            put("sources", JsonArray(sorted))
            put("byLanguage", byLanguage)
            put("byType", byType)
            put("providerStatus", JsonArray(statuses))
            put("count", count)
            put("cached", false)
            put("fetchedAt", clock().toString())
        }
    }

    // ---- providers --------------------------------------------------------------------------------------------------------------

    private suspend fun providers(slot: Slot): JsonObject {
        val m = slot.ext.manifest
        var result: JsonElement = JsonNull
        withSession(slot) { s ->
            if (!s.hasMethod("listProviders")) throw ExtError.NotImplemented("Bundle does not implement listProviders")
            result = s.invoke("listProviders", emptyList()).result
        }
        val list = (result as? JsonArray).orEmpty()
        val byKind = LinkedHashMap<String, Int>()
        list.forEach { p -> byKind.merge((p as? JsonObject)?.str("kind") ?: "unknown", 1, Int::plus) }
        return buildJsonObject {
            put("namespace", m.namespace)
            put("count", list.size)
            put("byKind", JsonObject(byKind.mapValues { JsonPrimitive(it.value) }))
            put("providers", JsonArray(list))
            put("fetchedAt", clock().toString())
        }
    }

    private suspend fun provider(slot: Slot, name: String, params: JsonObject): JsonObject {
        if (name.isBlank()) throw ExtError.NotFound("Unknown provider")
        var result: JsonElement = JsonNull
        withSession(slot) { s ->
            if (!s.hasMethod("runProvider")) throw ExtError.NotImplemented("Bundle does not implement runProvider")
            try {
                result = s.invoke("runProvider", listOf(JsonPrimitive(name), sourceOptions(params))).result
            } catch (e: ExtError.BundleError) {
                if (Regex("unknown provider", RegexOption.IGNORE_CASE).containsMatchIn(e.message.orEmpty())) {
                    throw ExtError.NotFound(e.message.orEmpty())
                }
                throw e
            }
        }
        val o = result as? JsonObject ?: JsonObject(emptyMap())
        val diag = o["diagnostic"] as? JsonObject
        val sources = (o["results"] as? JsonArray).orEmpty().mapNotNull { r ->
            (r as? JsonObject)?.let { SourceNormalizer.normalize(it, name, null) }
        }.let(SourceNormalizer::sort)
        val (byLanguage, byType, count) = SourceNormalizer.summarize(sources)
        return buildJsonObject {
            put("provider", o.str("provider") ?: name)
            put("kind", o.str("kind")?.let { JsonPrimitive(it) } ?: JsonNull)
            put("status", diag?.str("status") ?: if (sources.isEmpty()) "empty" else "ok")
            put("durationMs", diag?.get("durationMs") ?: JsonNull)
            put("attempts", diag?.get("attempts") ?: JsonNull)
            put("error", diag?.get("error") ?: JsonNull)
            put("count", count)
            put("sources", JsonArray(sources))
            put("byLanguage", byLanguage)
            put("byType", byType)
            put("fetchedAt", clock().toString())
        }
    }

    // ---- manga / novel / custom -------------------------------------------------------------------------------------------------

    private class Spec(val methods: List<String>, val required: List<String>, val args: (JsonObject) -> List<JsonElement>, val sources: Boolean = false)

    private val specs: Map<String, Spec> = mapOf(
        "manga/chapters" to Spec(listOf("debugMangaChapters", "getMangaChapters"), emptyList(), { listOf(it) }),
        "manga/pages" to Spec(listOf("getMangaPages"), listOf("chapterKey"), { listOf(it) }),
        "novel/search" to Spec(
            listOf("getNovelSearch"), listOf("query"),
            { listOf(buildJsonObject { put("query", it.str("query") ?: ""); it.str("provider")?.let { p -> put("provider", p) } }) },
        ),
        "novel/chapters" to Spec(listOf("debugNovelChapters", "getNovelChapters"), emptyList(), { listOf(it) }),
        "novel/content" to Spec(listOf("getNovelContent"), listOf("chapterKey"), { listOf(it) }),
        "custom/home" to Spec(listOf("customHome"), listOf("sourceId"), { listOf(JsonPrimitive(it.str("sourceId"))) }),
        "custom/search" to Spec(
            listOf("customSearch"), listOf("sourceId"),
            {
                listOf(
                    JsonPrimitive(it.str("sourceId")),
                    JsonPrimitive(it.str("q") ?: it.str("query") ?: ""),
                    JsonPrimitive(it.int("page") ?: 1),
                    parseFilters(it.str("filters")),
                )
            },
        ),
        "custom/info" to Spec(listOf("customInfo"), listOf("sourceId", "id"), { listOf(JsonPrimitive(it.str("sourceId")), JsonPrimitive(it.str("id"))) }),
        "custom/watch" to Spec(
            listOf("customWatch"), listOf("sourceId", "id", "episodeId"),
            { listOf(JsonPrimitive(it.str("sourceId")), JsonPrimitive(it.str("id")), JsonPrimitive(it.str("episodeId"))) },
            sources = true,
        ),
        "custom/read" to Spec(
            listOf("customRead"), listOf("sourceId", "id", "chapterId"),
            { listOf(JsonPrimitive(it.str("sourceId")), JsonPrimitive(it.str("id")), JsonPrimitive(it.str("chapterId"))) },
        ),
    )

    private suspend fun generic(slot: Slot, path: String, params: JsonObject): JsonObject {
        val spec = specs[path] ?: throw ExtError.NotFound("Unknown route: $path")
        spec.required.firstOrNull { params.str(it).isNullOrEmpty() }?.let { throw ExtError.BadRequest("Missing required param '$it'") }
        val m = slot.ext.manifest
        var result: JsonElement = JsonNull
        withSession(slot) { s ->
            val method = spec.methods.firstOrNull { s.hasMethod(it) }
                ?: throw ExtError.NotImplemented("Bundle does not implement ${spec.methods.first()}")
            result = s.invoke(method, spec.args(params)).result
        }
        val now = clock().toString()
        val o = result as? JsonObject ?: JsonObject(mapOf("result" to result))
        if (spec.sources) {
            val list = ((o["sources"] ?: o["results"]) as? JsonArray).orEmpty().mapNotNull {
                (it as? JsonObject)?.let { s -> SourceNormalizer.normalize(s, s.str("providerName") ?: m.id, null) }
            }.let(SourceNormalizer::sort)
            val (byLanguage, byType, _) = SourceNormalizer.summarize(list)
            return buildJsonObject {
                put("namespace", m.namespace); put("sourceId", params.str("sourceId") ?: "")
                put("sources", JsonArray(list)); put("byLanguage", byLanguage); put("byType", byType); put("fetchedAt", now)
            }
        }
        val extra = if (path == "custom/home") {
            JsonObject(
                mapOf(
                    "sourceId" to JsonPrimitive(params.str("sourceId") ?: ""),
                    "sections" to (o["sections"] ?: JsonArray(emptyList())),
                    "filters" to (o["filters"] ?: JsonNull),
                ),
            )
        } else {
            o
        }
        return SourceNormalizer.envelope(m.namespace, extra, now)
    }

    // ---- plumbing ---------------------------------------------------------------------------------------------------------------

    private suspend fun withSession(slot: Slot, block: suspend (JsSession) -> Unit) {
        slot.mutex.withLock {
            val ext = slot.ext
            var session = slot.session
            if (session == null) {
                session = engine.open(ext.manifest, ext.bundle, hostFor(ext.manifest))
                slot.session = session
            }
            try {
                block(session)
            } catch (e: ExtError.Timeout) {
                discard(slot); throw e
            } catch (e: ExtError.OutOfMemory) {
                discard(slot); throw e
            }
        }
    }

    private fun discard(slot: Slot) {
        slot.session?.runCatching { close() }
        slot.session = null
    }

    /** SourceOptions of the bundle API from the legacy query keys (routes.v1.json `queryParams.sourceOptions`). */
    internal fun sourceOptions(params: JsonObject): JsonObject = buildJsonObject {
        put("anilistId", params.num("anilistId")?.takeIf { it > 0 } ?: 0)
        put("titles", JsonArray(titles(params).map { JsonPrimitive(it) }))
        put("episode", params.num("episode") ?: 1)
        put("resolution", params.str("resolution") ?: "1080p")
        params.str("preferredLanguage")?.let { put("preferredLanguage", it) }
        params.str("preferredLanguages")?.let { csv ->
            put("preferredLanguages", JsonArray(csv.split(',').map { it.trim() }.filter { it.isNotEmpty() }.map { JsonPrimitive(it) }))
        }
        params.str("tatakaiId")?.let { put("tatakaiId", it) }
        params.num("episodeCount")?.let { put("episodeCount", it) }
        val po = LinkedHashMap<String, JsonElement>()
        for (k in listOf("maxConcurrency", "maxRetries", "retryDelayMs", "timeoutMs")) params.num(k)?.let { po[k] = JsonPrimitive(it) }
        po.putIfAbsent("timeoutMs", JsonPrimitive(DEFAULT_PROVIDER_TIMEOUT_MS))
        put("providerOptions", JsonObject(po))
    }

    private fun titles(params: JsonObject): List<String> {
        val el = params["titles[]"] ?: params["titles"] ?: return emptyList()
        return when (el) {
            is JsonArray -> el.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }
            is JsonPrimitive -> el.contentOrNull?.let { listOf(it) }.orEmpty()
            else -> emptyList()
        }.filter { it.isNotBlank() }
    }

    private fun parseFilters(text: String?): JsonElement =
        if (text.isNullOrBlank()) JsonObject(emptyMap()) else runCatching { Json.parseToJsonElement(text) }.getOrDefault(JsonObject(emptyMap()))

    private fun JsonObject.flag(key: String) = str(key) == "1" || str(key) == "true"

    private fun JsonObject.num(key: String): Int? = (this[key] as? JsonPrimitive)?.contentOrNull?.toDoubleOrNull()?.toInt()

    private fun canonical(params: JsonObject): String =
        params.entries.filter { it.key != "refresh" && it.key != "nocache" && it.key != "stream" }
            .sortedBy { it.key }.joinToString("&") { "${it.key}=${it.value}" }

    companion object {
        const val AGGREGATE_TTL_S = 600L
        const val TORRENT_TTL_S = 1800L
        const val DEFAULT_PROVIDER_TIMEOUT_MS = 20_000
        val CUSTOM_ROUTES = listOf("custom/home", "custom/search", "custom/info", "custom/watch", "custom/read")
    }
}
