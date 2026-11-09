// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkProfile
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkedDevice
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LinkHandoff
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.RemoteAction
import app.tatakai.next.model.RemotePlayerState
import app.tatakai.next.model.RemoteSession
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
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
import java.util.Locale
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
 * Phone-side extras of the Link discovery for :feature:link (wave B, link.discovery): the TOFU fingerprint announced by a PC and the
 * pairing with the code the PC shows. Implemented by [LinkRepositoryImpl] (Koin binding in linkModule).
 */
interface LinkDiscoveryAccess {
    /** Display fingerprint (`7A3F 91C2 …`) announced in the TXT `fp` of [pc]; null when the PC is no longer listed. */
    fun fingerprintOf(pc: DiscoveredPc): String?

    /** Pairs with a discovered PC: the user typed the one-time [code] shown on it, after confirming [fingerprintOf]. */
    suspend fun pairDiscovered(pc: DiscoveredPc, code: String, deviceName: String): Result<PairedPc>
}

/**
 * [LinkRepository] over [LinkClient]/[PairingClient]. Pairing by payload (QR / pasted) or, wave B, by a PC found over mDNS
 * ([discover], [LinkDiscoveryAccess]); remote control and hand-off over `POST /v1/control`; PC -> phone events by long-poll.
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
    /** android wave: epoch ms for pairedAtMs / lastSeenMs. */
    private val nowMs: () -> Long = System::currentTimeMillis,
    // --- parity wave B (16 §3.11, worker r) ---
    /** mDNS announcements of PCs (`NsdPcDiscovery` on device); null = no discovery (tests). */
    private val pcDiscovery: PcDiscovery? = null,
    /** App in the foreground: the `GET /v1/events` long-poll only runs while true; null = never polls (tests drive [pollEventsOnce]). */
    foreground: Flow<Boolean>? = null,
    private val handoffNotifier: LinkHandoffNotifier? = null,
) : LinkRepository, RemotePlaybackAccess, LinkDiscoveryAccess {
    private val _state = MutableStateFlow<LinkState>(
        vault.pairing()?.let { LinkState.Paired(it.toPc(), reachable = true) } ?: LinkState.Unpaired,
    )
    override val state: StateFlow<LinkState> = _state.asStateFlow()

    // --- android wave (12 §3.5 / §3.6) ---
    private val _linkedDevice = MutableStateFlow(readLinkedDevice())
    override val linkedDevice: StateFlow<LinkedDevice?> = _linkedDevice.asStateFlow()

    /** Last time lastSeenMs reached disk: a playback heartbeat every 4 s must not rewrite the file each time. */
    @Volatile private var lastSeenPersistedMs: Long = 0

    private class Connection(val identity: LinkIdentity, val http: OkHttpClient, val client: LinkClient)

    @Volatile private var connection: Connection? = null
    private val sessions = ConcurrentHashMap<String, RemoteSession>()

    // ------------------------------------------------------------------ parity wave B: discovery, remote control, hand-off, events

    private val scope = CoroutineScope(SupervisorJob() + io)
    private val found = ConcurrentHashMap<String, FoundPc>()
    private val _pendingHandoff = MutableStateFlow<LinkHandoff?>(null)
    override val pendingHandoff: StateFlow<LinkHandoff?> = _pendingHandoff.asStateFlow()

    init {
        if (foreground != null) {
            scope.launch {
                combine(foreground, state) { fg, s -> fg && s is LinkState.Paired }
                    .distinctUntilChanged()
                    .collectLatest { active -> if (active) eventsLoop() }
            }
        }
    }

    /** PCs announcing `_tatakai-link._tcp` (link.discovery); already-paired PC excluded by the screen. */
    override fun discover(): Flow<List<DiscoveredPc>> = pcDiscovery?.pcs()?.map { list ->
        list.forEach { found[key(it.pc)] = it }
        list.map { it.pc }
    } ?: emptyFlow()

    override fun fingerprintOf(pc: DiscoveredPc): String? = found[key(pc)]?.fingerprint?.let(DiscoveryWire::displayFingerprint)

    override suspend fun pairDiscovered(pc: DiscoveredPc, code: String, deviceName: String): Result<PairedPc> {
        val f = found[key(pc)] ?: return Result.failure(AppException(AppError.Unknown("link:discovery:gone")))
        val payload = DiscoveryWire.payload(f, code, clock()) ?: return Result.failure(AppException(AppError.Unknown("link:payload:InvalidCode")))
        return pair(payload, deviceName)
    }

    private fun key(pc: DiscoveredPc) = pc.instanceId ?: "${pc.host}:${pc.port}"

    override suspend fun remoteState(): Result<RemotePlayerState?> = call { c ->
        c.playerState().map { dto -> dto?.let(LinkWireMapping::playerState) }
    }

    override suspend fun remote(action: RemoteAction): Result<Unit> = call { c -> c.playerAction(action.id) }

    override suspend fun handoff(animeId: String, episode: Int, positionSec: Int): Result<Unit> =
        call { c -> c.handoff(animeId, episode, positionSec) }

    override fun dismissHandoff() {
        _pendingHandoff.value = null
    }

    /** Long-poll while active; errors back off (5 s, doubling up to 60 s). Cancelled when the app leaves the foreground. */
    private suspend fun eventsLoop() {
        var backoff = EVENTS_BACKOFF_MS
        while (true) {
            if (pollEventsOnce()) {
                backoff = EVENTS_BACKOFF_MS
            } else {
                delay(backoff)
                backoff = (backoff * 2).coerceAtMost(EVENTS_BACKOFF_MAX_MS)
            }
        }
    }

    /** One `GET /v1/events?wait=25`; true when the PC answered (with or without events). */
    internal suspend fun pollEventsOnce(): Boolean {
        val r = call { c -> c.events(ControlWire.MAX_WAIT_SECS) }
        val events = r.getOrNull() ?: return false
        val conn = connectionOrNull() ?: return true
        events.mapNotNull(LinkWireMapping::handoff).lastOrNull()?.let { h ->
            // The PC already started the session (send_to_device): register it so the player can open it by id.
            sessions[h.sessionId] = RemoteSession(
                sessionId = h.sessionId,
                generation = 1,
                playbackUrl = conn.client.playbackUrl("/v1/sessions/${h.sessionId}/master.m3u8"),
                durationSecs = null,
                heartbeatIntervalSecs = DEFAULT_HEARTBEAT_SECS,
                idleTimeoutSecs = DEFAULT_IDLE_SECS,
            )
            _pendingHandoff.value = h
            runCatching { handoffNotifier?.notify(h) }
        }
        return true
    }

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
                val now = nowMs()
                runCatching {
                    vault.saveMeta(StoredDeviceMeta(pairedAtMs = now, lastSeenMs = now, localDeviceName = LinkCsr.sanitizeDeviceName(deviceName)))
                }
                lastSeenPersistedMs = now
                _linkedDevice.value = readLinkedDevice()
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
        // android wave: the profile chosen on the Linked screen applies when the caller has no explicit one
        c.createSession(wire, profileId ?: _linkedDevice.value?.preferredProfile).map { dto ->
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
            _pendingHandoff.value = null
            val c = connectionOrNull()?.client
            if (c != null) {
                sessions.keys.toList().forEach { c.deleteSession(it) }
                c.forgetSelf() // best effort; if the PC is offline the certificate stays valid there until revoked in its UI
            }
            sessions.clear()
            connection?.http?.connectionPool?.evictAll()
            connection = null
            vault.clear()
            _linkedDevice.value = null
            _state.value = LinkState.Unpaired
        }
    }

    override suspend fun setPreferredProfile(profileId: String?) {
        withContext(io) {
            if (!vault.hasPairing()) return@withContext
            val meta = vault.meta() ?: StoredDeviceMeta()
            runCatching { vault.saveMeta(meta.copy(preferredProfile = profileId?.takeIf { it.isNotBlank() })) }
            _linkedDevice.value = readLinkedDevice()
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
        if (reachable) noteSeen()
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

    /** The PC answered: lastSeenMs now (memory always, disk at most once a minute). Runs on [io] (inside [call]). */
    private fun noteSeen() {
        val now = nowMs()
        val cur = _linkedDevice.value ?: return
        _linkedDevice.value = cur.copy(lastSeenMs = now)
        if (now - lastSeenPersistedMs < LAST_SEEN_PERSIST_MS) return
        lastSeenPersistedMs = now
        runCatching { vault.saveMeta((vault.meta() ?: StoredDeviceMeta()).copy(lastSeenMs = now)) }
    }

    private fun readLinkedDevice(): LinkedDevice? {
        val p = vault.pairing() ?: return null
        val meta = vault.meta()
        return LinkedDevice(
            deviceId = p.deviceId,
            serverName = p.serverName,
            host = p.host,
            port = p.port,
            fingerprint = displayFingerprint(p.caSha256),
            pairedAtMs = meta?.pairedAtMs,
            lastSeenMs = meta?.lastSeenMs,
            preferredProfile = meta?.preferredProfile,
            localDeviceName = meta?.localDeviceName,
        )
    }

    internal companion object {
        const val LAST_SEEN_PERSIST_MS = 60_000L
        const val EVENTS_BACKOFF_MS = 5_000L
        const val EVENTS_BACKOFF_MAX_MS = 60_000L
        const val DEFAULT_HEARTBEAT_SECS = 4
        const val DEFAULT_IDLE_SECS = 120

        /** `7a3f91c2...` -> `7A3F 91C2 ...`; null for an empty pin. */
        fun displayFingerprint(hex: String): String? {
            val clean = hex.filter { it.isLetterOrDigit() }.uppercase(Locale.ROOT)
            return clean.takeIf { it.isNotEmpty() }?.chunked(4)?.joinToString(" ")
        }
    }
}
