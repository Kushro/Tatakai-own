package app.tatakai.next.model

import app.tatakai.next.model.torrent.TorrentSettings
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.emptyFlow

// android wave (docs/redesign/12 §3.6): every member added to an EXISTING interface has a default body so the fakes in the
// feature tests keep compiling; the real implementations (owners in 12 §2) override them.

interface MetadataRepository {
    suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int = 1): FeedPage
    suspend fun search(kind: MediaKind, query: String, page: Int = 1): FeedPage
    suspend fun media(kind: MediaKind, ref: MediaRef): Media?
    suspend fun episodes(ref: MediaRef): List<Episode>

    // --- android wave (worker a) ---
    /** Size of the metadata response cache (Settings › Metadatos › caché). */
    suspend fun cacheStats(): MetadataCacheStats = MetadataCacheStats(entries = 0, bytes = 0)

    /** Settings › Datos y almacenamiento › Borrar caché. */
    suspend fun clearCache() {}
}

data class MetadataCacheStats(val entries: Int, val bytes: Long)

interface HistoryRepository {
    fun continueWatching(limit: Int = 20): Flow<List<HistoryEntry>>
    suspend fun record(entry: HistoryEntry)
    fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>>

    // --- android wave (worker a) ---
    /** Library › Historial: one row per watched episode, newest first (not grouped per series like [continueWatching]). */
    fun history(limit: Int = 200): Flow<List<HistoryEntry>> = continueWatching(limit)

    /** "Sincronizar ahora" (You account card, Library): push pending rows and pull remote ones now. Success without an account = no-op. */
    suspend fun syncNow(): Result<Unit> = Result.success(Unit)

    /** Epoch ms of the last successful sync; null = never / no account. */
    val lastSyncedAt: StateFlow<Long?> get() = NO_SYNC
}

interface AuthRepository {
    val session: StateFlow<UserSession?>
    suspend fun signIn(email: String, password: String): Result<Unit>
    suspend fun signOut()
}

interface SettingsRepository {
    val settings: StateFlow<AppSettings>
    suspend fun update(transform: (AppSettings) -> AppSettings)

    // --- android wave: convenience setters, implemented here on top of [update] (implementations need not override) ---
    suspend fun setPlayerEngine(engine: PlayerEngine) = update { it.copy(playerEngine = engine) }

    suspend fun setSourcePrefs(prefs: SourcePrefs) = update { it.copy(sourcePrefs = prefs) }

    suspend fun setTorrentSettings(settings: TorrentSettings) = update { it.copy(torrent = settings.sanitized()) }

    /** "Entiendo" of the background downloads warning. */
    suspend fun acknowledgeBackgroundDownloads() = update { it.copy(torrent = it.torrent.copy(backgroundAck = true)) }

    suspend fun setReduceMotion(enabled: Boolean) = update { it.copy(reduceMotion = enabled) }

    suspend fun setLanguage(language: AppLanguage) = update { it.copy(language = language) }
}

interface ExtensionRepository {
    val installed: StateFlow<List<InstalledExtension>>
    suspend fun installFromUri(uri: String): Result<InstalledExtension>
    suspend fun setEnabled(id: String, enabled: Boolean)
    suspend fun remove(id: String)
    suspend fun resolveStreams(media: Media, episode: Int): List<StreamSource>

    // --- android wave (worker e) ---
    /**
     * Every candidate (web from `stream`, torrent from `torrent` - also for custom/onlinestream extensions that list `torrent` explicitly in
     * apiServer.routes or expose `torrentAll`; the `type != TORRENT` filter is gone) of every enabled extension, with stored reliabilities,
     * ranked by `rank(candidates, settings.sourcePrefs, extEnabled)` of :core:release; best first, `recommended` on the first, `picked` from
     * [SourceStatsStore.pickedSource]. One failing extension never hides the others.
     */
    suspend fun resolveCandidates(media: Media, episode: Int): List<RankedSource> = emptyList()

    /** Player start / failure of a source ([sourceKey]): updates the reliability moving average. */
    suspend fun reportSourceResult(sourceKey: String, ok: Boolean) {}

    /** Collapsible per-extension log, newest last, capped (e.g. 200 lines). */
    fun logs(extensionId: String): Flow<List<ExtensionLogEntry>> = emptyFlow()

    suspend fun detail(extensionId: String): ExtensionDetail? = null

    /** Lazy Home rows from `custom/home` of extensions with custom sources. */
    suspend fun customHomeRows(): List<ExtensionHomeRow> = emptyList()
}

interface LinkRepository {
    val state: StateFlow<LinkState>

    /** M0: always empty (mDNS is M1); pairing is by QR / pasted payload. */
    fun discover(): Flow<List<DiscoveredPc>>
    suspend fun pair(payload: String, deviceName: String): Result<PairedPc>
    suspend fun capabilities(): Result<LinkCapabilities>
    suspend fun startSession(source: LinkSource, profileId: String?): Result<RemoteSession>
    suspend fun stop(sessionId: String)
    suspend fun forget()

    // --- android wave (worker a) ---
    /** The paired PC for the "Dispositivo enlazado" screen (null = unpaired). */
    val linkedDevice: StateFlow<LinkedDevice?> get() = NO_LINKED_DEVICE

    /** Enhance profile segmented of the Linked screen; null = the PC's default. Used by [startSession] when its profileId is null. */
    suspend fun setPreferredProfile(profileId: String?) {}
}

private val NO_SYNC: StateFlow<Long?> = MutableStateFlow<Long?>(null).asStateFlow()
private val NO_LINKED_DEVICE: StateFlow<LinkedDevice?> = MutableStateFlow<LinkedDevice?>(null).asStateFlow()
