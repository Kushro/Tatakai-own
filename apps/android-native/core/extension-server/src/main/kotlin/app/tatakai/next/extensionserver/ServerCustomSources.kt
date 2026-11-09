// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.extensions.ExternalCustomSources
import app.tatakai.next.extensions.ProviderParsers
import app.tatakai.next.extensions.SourceNormalizer
import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.ProviderItem
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker t; checklist ext.ecosystems.via-server,
 * ext.server-client.fallback, server.api.invoke / server.api.sources phone side): the custom sources of the linked PC extension servers
 * (Sora / Mangayomi / Seanime / Aniyomi / Mihon / CloudStream, and the PC's `.kai`, only through the server: plan D4) as provider pages.
 *
 * * `customSources` lists `GET /sources` of every enabled server (60 s cache, 8 s per server) as
 *   `CustomSourceRef(extensionId = "server:<serverId>/<extension>", sourceId = <server source id>)`; a server that does not answer
 *   contributes nothing and is marked offline (the local extensions keep working).
 * * The provider calls go through `POST /invoke`. The native desktop server takes the extension-host route names (`custom/home`,
 *   `custom/search`, `custom/info`, `custom/watch`, `custom/read`); the Node server takes its data ops (`browse`, `search`, `info`,
 *   `episodes` / `chapters`, `resolve`, `pages` / `novelContent`). Each call tries the route first and falls back to the op when the
 *   server refuses it (`400` / `403 data_operation_required` / `404`). Answers are parsed by [ProviderParsers].
 */
class ServerCustomSources(
    private val repository: ExtensionServerRepositoryImpl,
    private val client: ExtensionServerClient,
    private val clock: () -> Long = System::currentTimeMillis,
    private val listTimeoutMs: Long = LIST_TIMEOUT_MS,
    private val callTimeoutMs: Long = CALL_TIMEOUT_MS,
) : ExternalCustomSources {
    private var cached: Pair<Long, List<Pair<ExtensionServerConnection, ServerSource>>>? = null

    override fun owns(extensionId: String): Boolean = extensionId.startsWith(ServerCandidates.EXT_PREFIX)

    override suspend fun customSources(): List<CustomSourceRef> = rows().map { (conn, src) ->
        CustomSourceRef(
            extensionId = extensionIdOf(conn, src),
            extensionName = "${src.name.ifBlank { src.extensionId }} · ${conn.name}",
            sourceId = src.id,
            kind = src.kind.takeIf { it == "watch" || it == "read" || it == "novel" } ?: "watch",
            name = src.name.ifBlank { src.id },
            description = src.ecosystem.takeIf { it.isNotBlank() },
        )
    }

    override suspend fun home(extensionId: String, sourceId: String): List<ExtensionHomeRow> {
        val (conn, src) = resolve(extensionId, sourceId)
        val body = invokeFirst(conn, src.id, listOf("custom/home", "browse"), buildJsonObject { put("sourceId", src.id); put("page", 1) })
        return ProviderParsers.homeRows(extensionId, sourceId, src.name, body)
    }

    override suspend fun search(extensionId: String, sourceId: String, query: String, page: Int): List<ExtensionHomeItem> {
        val (conn, src) = resolve(extensionId, sourceId)
        val body = invokeFirst(
            conn, src.id, listOf("custom/search", "search"),
            buildJsonObject {
                put("sourceId", src.id)
                put("query", query)
                put("q", query)
                put("page", page)
            },
        )
        return ProviderParsers.cards(body)
    }

    override suspend fun info(extensionId: String, sourceId: String, itemId: String): ProviderItem? {
        val (conn, src) = resolve(extensionId, sourceId)
        val args = buildJsonObject {
            put("sourceId", src.id)
            put("id", itemId)
        }
        val body = invokeFirst(conn, src.id, listOf("custom/info", "info"), args)
        val kind = kindOf(src)
        val first = ProviderParsers.info(itemId, kind, body) ?: return null
        if (first.entries.isNotEmpty()) return first
        val listOp = when (kind) {
            "read" -> "chapters"
            "novel" -> "novelChapters"
            else -> "episodes"
        }
        val extra = runCatchingNonCancel { invokeFirst(conn, src.id, listOf(listOp), args) } ?: return first
        return ProviderParsers.info(itemId, kind, body, extra) ?: first
    }

    override suspend fun watch(extensionId: String, sourceId: String, itemId: String, entryId: String): List<StreamSource> {
        val (conn, src) = resolve(extensionId, sourceId)
        val body = invokeFirst(
            conn, src.id, listOf("custom/watch", "resolve"),
            buildJsonObject {
                put("sourceId", src.id)
                put("id", entryId)
                put("titleId", itemId)
                put("episodeId", entryId)
                put("url", entryId)
            },
        )
        val sources: JsonElement = when (body) {
            is JsonArray -> body
            is JsonObject -> body["sources"] ?: body["streams"] ?: body["result"] ?: JsonArray(emptyList())
            else -> JsonArray(emptyList())
        }
        // The server already proxified the media URLs (`/media/<ticket>`): its headers are not needed by the player.
        val normalized = (sources as? JsonArray).orEmpty().mapNotNull { el ->
            (el as? JsonObject)?.let { SourceNormalizer.normalize(it, it.text("providerName") ?: src.name, null) }
        }
        return SourceNormalizer.toStreamSources(JsonObject(mapOf("sources" to JsonArray(normalized))), extensionId)
    }

    override suspend fun read(extensionId: String, sourceId: String, itemId: String, entryId: String): List<ChapterPage> {
        val (conn, src) = resolve(extensionId, sourceId)
        val kind = kindOf(src)
        val ops = if (kind == "novel") listOf("custom/read", "novelContent") else listOf("custom/read", "pages")
        val body = invokeFirst(
            conn, src.id, ops,
            buildJsonObject {
                put("sourceId", src.id)
                put("id", entryId)
                put("titleId", itemId)
                put("chapterId", entryId)
                put("url", entryId)
            },
        )
        return ProviderParsers.pages(kind, body)
    }

    // ---- plumbing ---------------------------------------------------------------------------------------------------------------

    private fun kindOf(src: ServerSource): String = src.kind.takeIf { it == "read" || it == "novel" } ?: "watch"

    /** Sources of every enabled server (cached [CACHE_TTL_MS]); each server bounded by [listTimeoutMs]. */
    private suspend fun rows(): List<Pair<ExtensionServerConnection, ServerSource>> {
        cached?.takeIf { clock() - it.first < CACHE_TTL_MS }?.let { return it.second }
        val servers = repository.enabledServers()
        if (servers.isEmpty()) return emptyList()
        val all = coroutineScope {
            servers.map { conn ->
                async {
                    try {
                        val list = withTimeoutOrNull(listTimeoutMs) { client.sources(conn) }
                        if (list == null) {
                            repository.setStatus(conn.id, ServerStatus.Offline)
                            emptyList()
                        } else {
                            repository.noteSuccess(conn)
                            list.filter { it.enabled && it.available }.map { conn to it }
                        }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        repository.noteFailure(conn, e)
                        emptyList()
                    }
                }
            }.awaitAll().flatten()
        }
        cached = clock() to all
        return all
    }

    private suspend fun resolve(extensionId: String, sourceId: String): Pair<ExtensionServerConnection, ServerSource> {
        val found = rows().firstOrNull { (conn, src) -> src.id == sourceId && extensionIdOf(conn, src) == extensionId }
            ?: run {
                cached = null
                rows().firstOrNull { (conn, src) -> src.id == sourceId && extensionIdOf(conn, src) == extensionId }
            }
        return found ?: throw ExtensionServerException(ExtensionServerException.UNREACHABLE)
    }

    /** Tries [ops] in order; a refusal of the op itself moves to the next one, anything else is the answer (or the error). */
    private suspend fun invokeFirst(conn: ExtensionServerConnection, sourceId: String, ops: List<String>, args: JsonObject): JsonElement {
        var last: Exception? = null
        for (op in ops) {
            try {
                val result = withTimeoutOrNull(callTimeoutMs) { client.invoke(conn, sourceId, op, args) }
                    ?: throw ExtensionServerException(ExtensionServerException.UNREACHABLE)
                repository.noteSuccess(conn)
                return result
            } catch (e: ExtensionServerException) {
                last = e
                if (!refusedOp(e)) {
                    repository.noteFailure(conn, e)
                    throw e
                }
            }
        }
        throw last ?: ExtensionServerException(ExtensionServerException.BAD_RESPONSE)
    }

    private suspend fun <T> runCatchingNonCancel(block: suspend () -> T): T? = try {
        block()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }

    private fun JsonObject.text(key: String): String? = (this[key] as? JsonPrimitive)?.takeIf { it.isString }?.content

    companion object {
        const val CACHE_TTL_MS = 60_000L
        const val LIST_TIMEOUT_MS = 8_000L
        const val CALL_TIMEOUT_MS = 30_000L

        /** `server:<serverId>/<extension>`: the extension part is the server's extension id (or the source id without its variant). */
        fun extensionIdOf(conn: ExtensionServerConnection, src: ServerSource): String {
            val ext = src.extensionId.ifBlank { src.id.substringAfter('/').substringBefore('~') }
            return "${ServerCandidates.EXT_PREFIX}${conn.serverId}/${ext.replace("|", "_")}"
        }

        /** The server refused the op name itself (not the data): try the next spelling. */
        fun refusedOp(e: ExtensionServerException): Boolean =
            e.code == "data_operation_required" || e.code == "not_found" || e.code == "bad_request" ||
                e.code == "unknown_route" || e.status == 400 || e.status == 404 || (e.status == 403 && e.code != "forbidden")
    }
}
