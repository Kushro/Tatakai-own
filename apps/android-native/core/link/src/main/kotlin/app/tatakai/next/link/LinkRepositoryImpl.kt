// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkProfile
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.RemoteSession
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import java.util.concurrent.ConcurrentHashMap

/** What the player needs from the Link without knowing about certificates: the mTLS client and the heartbeat. */
interface RemotePlaybackAccess {
    /** OkHttp client that presents the client certificate and trusts only the pinned CA; null when unpaired. Use it for the HLS URL. */
    fun playbackClient(): OkHttpClient?

    /** The session created through [LinkRepository.startSession], if still open. */
    fun session(sessionId: String): RemoteSession?

    /** `POST /v1/sessions/{id}/position` (call every `heartbeatIntervalSecs`, 4 s). */
    suspend fun heartbeat(sessionId: String, positionSecs: Double, paused: Boolean): Result<Unit>
}

/**
 * [LinkRepository] over [LinkClient]/[PairingClient]. M0: pairing only by payload (QR / pasted), no mDNS ([discover] is empty).
 * The identity (sealed key + certificates) is loaded lazily, so constructing the repository never touches the Keystore.
 */
class LinkRepositoryImpl(
    private val vault: LinkKeyVault,
    private val pairingClient: PairingClient = PairingClient(),
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val httpFactory: (LinkIdentity) -> OkHttpClient = { id ->
        LinkCrypto.httpClient(
            id.pairing.host, id.pairing.caSha256,
            LinkCrypto.ClientKeyManager(id.privateKey, arrayOf(id.certificate)),
        )
    },
    private val clock: () -> Long = { System.currentTimeMillis() / 1000 },
) : LinkRepository, RemotePlaybackAccess {
    private val _state = MutableStateFlow<LinkState>(
        vault.pairing()?.let { LinkState.Paired(it.toPc(), reachable = true) } ?: LinkState.Unpaired,
    )
    override val state: StateFlow<LinkState> = _state.asStateFlow()

    private class Connection(val identity: LinkIdentity, val http: OkHttpClient, val client: LinkClient)

    @Volatile private var connection: Connection? = null
    private val sessions = ConcurrentHashMap<String, RemoteSession>()

    override fun discover(): Flow<List<DiscoveredPc>> = emptyFlow()

    override suspend fun pair(payload: String, deviceName: String): Result<PairedPc> = withContext(io) {
        val parsed = when (val r = PairingPayload.parse(payload, clock())) {
            is PayloadResult.Valid -> r.payload
            is PayloadResult.Invalid -> return@withContext Result.failure(AppException(AppError.Unknown("link:payload:${r.error::class.simpleName}")))
        }
        val before = _state.value
        _state.value = LinkState.Pairing
        val keyPair = LinkCrypto.generateKeyPair()
        when (val result = pairingClient.pair(parsed, deviceName, keyPair)) {
            is LinkResult.Failure -> {
                _state.value = before
                Result.failure(result.error.toAppException())
            }
            is LinkResult.Ok -> try {
                val grant = result.value
                val stored = StoredPairing(grant.deviceId, parsed.pcName.ifBlank { parsed.host }, parsed.host, parsed.port, parsed.caSha256)
                vault.save(stored, LinkCrypto.privateKeyPem(keyPair.private), LinkCrypto.certificatePem(grant.certificate), LinkCrypto.certificatePem(grant.ca))
                sessions.clear()
                connection = null
                _state.value = LinkState.Paired(stored.toPc(), reachable = true)
                Result.success(stored.toPc())
            } catch (e: Exception) {
                _state.value = before
                Result.failure(AppException(AppError.Unknown("link:store"), e))
            }
        }
    }

    override suspend fun capabilities(): Result<LinkCapabilities> = call { c ->
        c.capabilities().map { dto ->
            LinkCapabilities(
                serverName = dto.serverName,
                serverVersion = dto.serverVersion,
                instanceId = dto.instanceId,
                profiles = dto.profiles.map {
                    LinkProfile(
                        id = it.id,
                        label = it.label,
                        upscale = it.upscale != "none",
                        frameGeneration = it.fg != "none",
                        outputWidth = it.output.width,
                        outputHeight = it.output.height,
                        measuredSustain = (it.measuredSustain as? JsonPrimitive)?.let { p -> p.booleanOrNull?.toString() ?: p.contentOrNull } ?: "unknown",
                    )
                },
                defaultProfile = dto.defaultProfile,
                clientFrameGenerationAllowed = dto.clientFg == "allowed",
            )
        }
    }

    override suspend fun startSession(source: LinkSource, profileId: String?): Result<RemoteSession> = call { c ->
        val wire = when (source) {
            is LinkSource.Library -> SessionSource.Library(source.itemId)
            is LinkSource.Url -> SessionSource.Url(source.url, source.headers)
        }
        c.createSession(wire, profileId).map { dto ->
            RemoteSession(
                sessionId = dto.sessionId,
                generation = dto.generation,
                playbackUrl = c.playbackUrl(dto.playbackPath),
                durationSecs = dto.durationSecs,
                heartbeatIntervalSecs = dto.heartbeatIntervalSecs,
                idleTimeoutSecs = dto.idleTimeoutSecs,
            ).also { sessions[it.sessionId] = it }
        }
    }

    override suspend fun stop(sessionId: String) {
        sessions.remove(sessionId)
        val c = connectionOrNull()?.client ?: return
        withContext(io) { c.deleteSession(sessionId) } // best effort: the PC closes idle sessions after 120 s anyway
    }

    override suspend fun forget() {
        withContext(io) {
            val c = connectionOrNull()?.client
            if (c != null) {
                sessions.keys.toList().forEach { c.deleteSession(it) }
                c.forgetSelf() // best effort; if the PC is offline the certificate stays valid there until revoked in its UI
            }
            sessions.clear()
            connection?.http?.connectionPool?.evictAll()
            connection = null
            vault.clear()
            _state.value = LinkState.Unpaired
        }
    }

    override fun playbackClient(): OkHttpClient? = connectionOrNull()?.http

    override fun session(sessionId: String): RemoteSession? = sessions[sessionId]

    override suspend fun heartbeat(sessionId: String, positionSecs: Double, paused: Boolean): Result<Unit> =
        call { c -> c.heartbeat(sessionId, positionSecs, paused) }

    // ---------------------------------------------------------------------------------------------------------------------------

    private suspend fun <T> call(block: suspend (LinkClient) -> LinkResult<T>): Result<T> = withContext(io) {
        val conn = connectionOrNull() ?: return@withContext Result.failure(LinkError.NotPaired.toAppException())
        when (val r = block(conn.client)) {
            is LinkResult.Ok -> {
                markReachable(conn, true)
                Result.success(r.value)
            }
            is LinkResult.Failure -> {
                noteFailure(conn, r.error)
                Result.failure(r.error.toAppException())
            }
        }
    }

    private fun noteFailure(conn: Connection, error: LinkError) {
        when (error) {
            is LinkError.Transport.Refused, is LinkError.Transport.Timeout -> markReachable(conn, false)
            is LinkError.Transport.Tls.Revoked -> _state.value = LinkState.Failed("revoked")
            is LinkError.Transport.Tls.Expired -> _state.value = LinkState.Failed("expired")
            is LinkError.Api -> if (error.error == LinkApiError.DeviceNotAuthorized) _state.value = LinkState.Failed("revoked") else markReachable(conn, true)
            else -> Unit
        }
    }

    private fun markReachable(conn: Connection, reachable: Boolean) {
        val cur = _state.value
        if (cur is LinkState.Paired && cur.reachable != reachable) _state.value = cur.copy(reachable = reachable)
        else if (cur is LinkState.Failed && reachable) _state.value = LinkState.Paired(conn.identity.pairing.toPc(), true)
    }

    private fun connectionOrNull(): Connection? {
        connection?.let { return it }
        synchronized(this) {
            connection?.let { return it }
            val identity = vault.load() ?: return null
            val http = httpFactory(identity)
            val client = LinkClient("https://${identity.pairing.host}:${identity.pairing.port}".toHttpUrl(), http)
            return Connection(identity, http, client).also { connection = it }
        }
    }

    private fun StoredPairing.toPc() = PairedPc(deviceId, serverName, host, port)
}
