// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import java.util.UUID

/** The PC extension servers linked to this phone (docs/redesign/13 §3.10). */
interface ExtensionServerRepository {
    val servers: StateFlow<List<ExtensionServerConnection>>

    /** False until the stored servers have been read (loading state of the screen). */
    val loaded: StateFlow<Boolean>

    /** Last observed reachability per connection id (memory only; [refresh] updates it). */
    val status: StateFlow<Map<String, ServerStatus>>

    /** Pairs with the one-time code of a scanned / pasted / opened link. Throws [ExtensionServerException]. */
    suspend fun connectWithLink(link: PairingLink): ExtensionServerConnection

    /** Viewer login with the server's optional viewer password. Throws [ExtensionServerException]. */
    suspend fun connectWithPassword(url: String, password: String): ExtensionServerConnection

    suspend fun setEnabled(id: String, enabled: Boolean)

    /** Forgets the server; logs this device out first (best effort, short timeout). */
    suspend fun remove(id: String)

    /** Health + source count of one server (or all when [id] is null). */
    suspend fun refresh(id: String? = null)

    /**
     * parity wave B (16 §3.4, worker t; checklist server.api.extensions): the extensions installed on server [id] (`GET /extensions`),
     * falling back to the extensions named by its `/sources` when the server keeps the list for its owner. Throws [ExtensionServerException].
     */
    suspend fun extensions(id: String): List<ServerExtension> = emptyList()
}

/**
 * [ExtensionServerRepository] over [ExtensionServerClient] and [ExtensionServerStore]. The store is read off the main thread at
 * construction (it may touch the Keystore), so constructing this class never blocks. Nothing here logs; tokens stay in the store and
 * in the `Authorization` header.
 */
class ExtensionServerRepositoryImpl(
    private val store: ExtensionServerStore,
    private val client: ExtensionServerClient,
    private val deviceLabel: String,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val newId: () -> String = { UUID.randomUUID().toString() },
) : ExtensionServerRepository {
    private val scope = CoroutineScope(SupervisorJob() + io)
    private val lock = Mutex()
    private val ready = CompletableDeferred<Unit>()
    private val _servers = MutableStateFlow<List<ExtensionServerConnection>>(emptyList())
    override val servers: StateFlow<List<ExtensionServerConnection>> = _servers.asStateFlow()
    private val _loaded = MutableStateFlow(false)
    override val loaded: StateFlow<Boolean> = _loaded.asStateFlow()
    private val _status = MutableStateFlow<Map<String, ServerStatus>>(emptyMap())
    override val status: StateFlow<Map<String, ServerStatus>> = _status.asStateFlow()

    init {
        scope.launch {
            runCatching { _servers.value = store.read() }
            ready.complete(Unit)
            _loaded.value = true
        }
    }

    /** Enabled connections, after the store has been read. */
    suspend fun enabledServers(): List<ExtensionServerConnection> {
        ready.await()
        return _servers.value.filter { it.enabled }
    }

    override suspend fun connectWithLink(link: PairingLink): ExtensionServerConnection = withContext(io) {
        HostGuard.require(link.server)
        val health = checkedHealth(link.server)
        val token = client.pair(link.server, link.code, deviceLabel)
        save(link.server, health, token)
    }

    override suspend fun connectWithPassword(url: String, password: String): ExtensionServerConnection = withContext(io) {
        val origin = HostGuard.originOf(url) ?: throw ExtensionServerException(ExtensionServerException.INVALID_URL)
        HostGuard.require(origin)
        val health = checkedHealth(origin)
        val token = client.login(origin, password, deviceLabel)
        save(origin, health, token)
    }

    override suspend fun setEnabled(id: String, enabled: Boolean) {
        mutate { list -> list.map { if (it.id == id) it.copy(enabled = enabled) else it } }
        if (!enabled) setStatus(id, ServerStatus.Unknown)
    }

    override suspend fun remove(id: String) {
        ready.await()
        val conn = _servers.value.firstOrNull { it.id == id } ?: return
        withContext(io) {
            // The PC drops the device token; if it is offline the token stays listed there until revoked in its dashboard.
            withTimeoutOrNull(LOGOUT_TIMEOUT_MS) { runCatchingNonCancel { client.logout(conn) } }
        }
        mutate { list -> list.filterNot { it.id == id } }
        _status.update { it - id }
    }

    override suspend fun refresh(id: String?) {
        ready.await()
        val targets = _servers.value.filter { it.enabled && (id == null || it.id == id) }
        coroutineScope {
            targets.map { conn -> async(io) { refreshOne(conn) } }.awaitAll()
        }
    }

    override suspend fun extensions(id: String): List<ServerExtension> = withContext(io) {
        ready.await()
        val conn = _servers.value.firstOrNull { it.id == id } ?: throw ExtensionServerException(ExtensionServerException.INVALID_URL)
        try {
            val list = try {
                client.extensions(conn)
            } catch (e: ExtensionServerException) {
                if (e.status != 403 && e.status != 404) throw e
                // The Node server lists extensions to its owner only: rebuild the list from the viewer-visible sources.
                client.sources(conn).groupBy { it.extensionId.ifBlank { it.id } }.map { (extId, rows) ->
                    val first = rows.first()
                    ServerExtension(
                        id = extId,
                        name = first.name.substringBefore(" · ").ifBlank { extId },
                        ecosystem = first.ecosystem,
                        kind = rows.joinToString(",") { it.kind },
                        enabled = rows.any { it.enabled },
                    )
                }
            }
            noteSuccess(conn)
            list
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            noteFailure(conn, e)
            throw e
        }
    }

    // ---- used by ServerCandidateSource ------------------------------------------------------------------------------------------

    fun setStatus(id: String, status: ServerStatus) {
        _status.update { it + (id to status) }
    }

    /** A call succeeded: online, keeping the source count of the last [refresh]. */
    fun noteSuccess(conn: ExtensionServerConnection) {
        _status.update { map -> if (map[conn.id] is ServerStatus.Online) map else map + (conn.id to ServerStatus.Online(conn.name, null)) }
    }

    /** Status after a failed call: 401 -> Unauthorized, transport -> Offline, any other server error leaves it online. */
    fun noteFailure(conn: ExtensionServerConnection, e: Throwable) {
        val status = when {
            e is ExtensionServerException && e.unauthorized -> ServerStatus.Unauthorized
            e is ExtensionServerException && e.code == ExtensionServerException.UNREACHABLE -> ServerStatus.Offline
            else -> return
        }
        setStatus(conn.id, status)
    }

    // ---- plumbing ---------------------------------------------------------------------------------------------------------------

    private suspend fun refreshOne(conn: ExtensionServerConnection) {
        setStatus(conn.id, ServerStatus.Checking)
        try {
            val health = withTimeoutOrNull(REFRESH_TIMEOUT_MS) { client.health(conn.url) }
            if (health == null) {
                setStatus(conn.id, ServerStatus.Offline)
                return
            }
            val count = withTimeoutOrNull(REFRESH_TIMEOUT_MS) { client.sources(conn).count { it.available && it.kind == "watch" } }
            setStatus(conn.id, ServerStatus.Online(health.name.ifBlank { conn.name }, count))
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            setStatus(conn.id, ServerStatus.Offline)
            noteFailure(conn, e)
        }
    }

    private suspend fun checkedHealth(origin: String): ServerHealth {
        val health = client.health(origin)
        if (health.protocolVersion != PROTOCOL_VERSION) throw ExtensionServerException(ExtensionServerException.INCOMPATIBLE)
        if (health.setupRequired || health.serverId.isNullOrBlank()) throw ExtensionServerException(ExtensionServerException.SETUP_REQUIRED)
        return health
    }

    private suspend fun save(origin: String, health: ServerHealth, token: ServerToken): ExtensionServerConnection {
        if (token.token.isBlank()) throw ExtensionServerException(ExtensionServerException.BAD_RESPONSE)
        val serverId = health.serverId.orEmpty()
        var saved: ExtensionServerConnection? = null
        mutate { list ->
            val existing = list.firstOrNull { it.serverId == serverId && it.url == origin }
            val conn = ExtensionServerConnection(
                id = existing?.id ?: newId(),
                serverId = serverId,
                url = origin,
                name = health.name.ifBlank { origin }.take(MAX_NAME),
                token = token.token,
                enabled = true,
            )
            saved = conn
            list.filterNot { it.id == conn.id } + conn
        }
        val conn = checkNotNull(saved)
        setStatus(conn.id, ServerStatus.Online(conn.name, null))
        return conn
    }

    private suspend fun mutate(change: (List<ExtensionServerConnection>) -> List<ExtensionServerConnection>) {
        ready.await()
        lock.withLock {
            val next = change(_servers.value)
            withContext(io) { store.write(next) }
            _servers.value = next
        }
    }

    private suspend fun <T> runCatchingNonCancel(block: suspend () -> T): T? = try {
        block()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }

    companion object {
        const val PROTOCOL_VERSION = 1
        const val LOGOUT_TIMEOUT_MS = 4_000L
        const val REFRESH_TIMEOUT_MS = 8_000L
        private const val MAX_NAME = 60
    }
}
