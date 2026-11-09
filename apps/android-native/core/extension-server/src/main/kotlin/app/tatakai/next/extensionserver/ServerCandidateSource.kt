// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.extensions.ExternalCandidateSource
import app.tatakai.next.model.Media
import app.tatakai.next.model.SourceCandidate
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * The linked PC servers as one more provider group of `ExtensionRepositoryImpl.resolveCandidates` (docs/redesign/13 §3.10).
 *
 * Every enabled server is asked in parallel through the progressive `/resolve` stream. Each server has [budgetMs]: when it runs out the
 * latest snapshot received so far is used (every NDJSON line is a complete snapshot), so a slow provider on the PC costs at most that
 * budget and never hides what already arrived. A server that is down fails fast (connect timeout) and contributes nothing: the
 * installed extensions keep working (the "offline fallback" of the D8 copy).
 *
 * Non-empty complete results are reused for [cacheTtlMs] (Details resolves, then the player resolves again on Watch).
 */
class ServerCandidateSource(
    private val repository: ExtensionServerRepositoryImpl,
    private val client: ExtensionServerClient,
    private val budgetMs: Long = SERVER_BUDGET_MS,
    private val cacheTtlMs: Long = CACHE_TTL_MS,
    private val clock: () -> Long = System::currentTimeMillis,
) : ExternalCandidateSource {
    private val cache = LinkedHashMap<String, Pair<Long, List<SourceCandidate>>>()

    override val available: Flow<Boolean> = repository.servers.map { list -> list.any { it.enabled } }

    override suspend fun candidates(media: Media, episode: Int): List<SourceCandidate> {
        val servers = repository.enabledServers()
        if (servers.isEmpty()) return emptyList()
        val request = requestOf(media, episode)
        return coroutineScope {
            servers.map { conn -> async { fromServer(conn, media, episode, request) } }.awaitAll().flatten()
        }
    }

    private suspend fun fromServer(conn: ExtensionServerConnection, media: Media, episode: Int, request: JsonObject): List<SourceCandidate> {
        val key = "${conn.id}|${conn.token.hashCode()}|${media.ref}|$episode"
        synchronized(cache) {
            cache[key]?.takeIf { clock() - it.first < cacheTtlMs }?.let { return it.second }
        }
        var last: ResolveSnapshot? = null
        var finished = false
        try {
            withTimeoutOrNull(budgetMs) {
                client.resolve(conn, request).collect { last = it }
                finished = true
            }
            repository.noteSuccess(conn)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            repository.noteFailure(conn, e)
        }
        val snapshot = last ?: return emptyList()
        val mapped = ServerCandidates.map(conn, snapshot)
        if (finished && mapped.isNotEmpty()) synchronized(cache) {
            cache[key] = clock() to mapped
            while (cache.size > CACHE_MAX) cache.remove(cache.keys.first())
        }
        return mapped
    }

    companion object {
        /** The server's own per-request limit is 25 s (`settings.timeoutMs` max); a little slack for the stream to close. */
        const val SERVER_BUDGET_MS = 30_000L
        const val CACHE_TTL_MS = 3 * 60_000L
        private const val CACHE_MAX = 16

        /** Same body the classic client sends (`{ anilistId, title, episode }`), plus the other titles and the MAL id when known. */
        fun requestOf(media: Media, episode: Int): JsonObject = buildJsonObject {
            media.ref.anilistId?.let { put("anilistId", it) }
            media.ref.malId?.let { put("malId", it) }
            put("title", media.title)
            val titles = (listOf(media.title) + media.titles.values).filter { it.isNotBlank() }.distinct()
            put("titles", JsonArray(titles.map { JsonPrimitive(it) }))
            put("episode", episode)
        }
    }
}
