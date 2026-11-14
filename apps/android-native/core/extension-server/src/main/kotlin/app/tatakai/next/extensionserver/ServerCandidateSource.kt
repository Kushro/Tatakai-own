// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.extensions.ExternalCandidateSource
import app.tatakai.next.model.Media
import app.tatakai.next.model.SourceCandidate
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.channelFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.transformWhile
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

/**
 * The linked PC servers as one more provider group of `ExtensionRepositoryImpl.resolveCandidates` (docs/redesign/13 §3.10).
 *
 * Every enabled server is asked in parallel through the progressive `/resolve` stream (every NDJSON line is a complete snapshot).
 * BUG-011 / BUG-019: one request per (server, media, episode) runs in [scope] up to [lateBudgetMs] and is SHARED by everyone who asks
 * meanwhile (Details resolves, then the player resolves again), so a slow provider on the PC (Toko: ~31 s on a first resolve) is never
 * cut off and never asked twice:
 * - [candidates] waits at most [budgetMs] and returns the latest snapshot (the request keeps running and fills the cache when it ends);
 * - [candidatesFlow] emits every snapshot as it arrives until the request ends (the source pickers show results progressively).
 * A server that is down fails fast (connect timeout) and contributes nothing: the installed extensions keep working.
 *
 * Non-empty complete results are reused for [cacheTtlMs].
 */
class ServerCandidateSource(
    private val repository: ExtensionServerRepositoryImpl,
    private val client: ExtensionServerClient,
    private val budgetMs: Long = SERVER_BUDGET_MS,
    private val cacheTtlMs: Long = CACHE_TTL_MS,
    private val clock: () -> Long = System::currentTimeMillis,
    private val lateBudgetMs: Long = LATE_BUDGET_MS,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO),
) : ExternalCandidateSource {
    private val cache = LinkedHashMap<String, Pair<Long, List<SourceCandidate>>>()

    /** One running `/resolve`: the latest mapped snapshot and whether the stream ended. */
    private data class Progress(val candidates: List<SourceCandidate> = emptyList(), val done: Boolean = false)

    private val inflight = HashMap<String, MutableStateFlow<Progress>>()

    override val available: Flow<Boolean> = repository.servers.map { list -> list.any { it.enabled } }

    override suspend fun candidates(media: Media, episode: Int): List<SourceCandidate> {
        val servers = repository.enabledServers()
        if (servers.isEmpty()) return emptyList()
        val request = requestOf(media, episode)
        return coroutineScope {
            servers.map { conn ->
                async {
                    val progress = progressOf(conn, media, episode, request)
                    withTimeoutOrNull(budgetMs) { progress.first { it.done } }?.candidates ?: progress.value.candidates
                }
            }.awaitAll().flatten()
        }
    }

    override fun candidatesFlow(media: Media, episode: Int): Flow<List<SourceCandidate>> = channelFlow {
        val servers = repository.enabledServers()
        if (servers.isEmpty()) {
            send(emptyList())
            return@channelFlow
        }
        val request = requestOf(media, episode)
        val latest = Array(servers.size) { emptyList<SourceCandidate>() }
        val lock = Any()
        coroutineScope {
            servers.forEachIndexed { i, conn ->
                launch {
                    val progress = progressOf(conn, media, episode, request)
                    progress.transformWhile { emit(it); !it.done }.collect { p ->
                        val all = synchronized(lock) {
                            latest[i] = p.candidates
                            latest.toList().flatten()
                        }
                        send(all)
                    }
                }
            }
        }
    }

    /** The cached result as a finished [Progress], the running request, or a new one started in [scope]. */
    private fun progressOf(conn: ExtensionServerConnection, media: Media, episode: Int, request: JsonObject): StateFlow<Progress> {
        val key = "${conn.id}|${conn.token.hashCode()}|${media.ref}|$episode"
        synchronized(cache) {
            cache[key]?.takeIf { clock() - it.first < cacheTtlMs }?.let { return MutableStateFlow(Progress(it.second, done = true)) }
        }
        synchronized(inflight) {
            inflight[key]?.let { return it }
            val state = MutableStateFlow(Progress())
            inflight[key] = state
            scope.launch {
                try {
                    run(conn, request, state)
                } finally {
                    // Cache first, then forget the request: a caller arriving in between finds one or the other, never neither.
                    val last = state.value
                    if (last.candidates.isNotEmpty() && last.done) synchronized(cache) {
                        cache[key] = clock() to last.candidates
                        while (cache.size > CACHE_MAX) cache.remove(cache.keys.first())
                    }
                    synchronized(inflight) { inflight.remove(key) }
                    state.value = last.copy(done = true)
                }
            }
            return state
        }
    }

    private suspend fun run(conn: ExtensionServerConnection, request: JsonObject, state: MutableStateFlow<Progress>) {
        var finished = false
        try {
            withTimeoutOrNull(lateBudgetMs) {
                client.resolve(conn, request).collect { snapshot -> state.value = Progress(ServerCandidates.map(conn, snapshot)) }
                finished = true
            }
            repository.noteSuccess(conn)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            repository.noteFailure(conn, e)
        }
        // Only a stream that ended by itself is complete (a cut one must not be cached as the whole answer).
        if (finished) state.value = state.value.copy(done = true)
    }

    companion object {
        /** What a caller that needs the whole list at once ([candidates], Auto mode) waits; the request itself goes on. */
        const val SERVER_BUDGET_MS = 30_000L

        /** Hard cap of one `/resolve` (the PC's providers can take > 30 s on a first resolve: BUG-011 measured 31-32 s for Toko). */
        const val LATE_BUDGET_MS = 90_000L
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

