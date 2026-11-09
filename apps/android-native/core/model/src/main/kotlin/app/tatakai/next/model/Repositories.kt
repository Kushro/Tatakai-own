package app.tatakai.next.model

import app.tatakai.next.model.torrent.TorrentSettings
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.flowOf

// android wave (docs/redesign/12 §3.6): every member added to an EXISTING interface has a default body so the fakes in the
// feature tests keep compiling; the real implementations (owners in 12 §2) override them.
// parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3): same rule. New interfaces of the wave live in their own files
// (Lists.kt: ListsRepository; Diagnostics.kt: LogRepository, UpdateRepository, SecretStore, AppReset; torrent/Debrid.kt: DebridRepository).
// 2026-11-09 amendment (plan 14 §0.1 #9): the Supabase `AuthRepository` / `UserSession` are gone. Identity = AniList and/or MAL
// (`ListsRepository.accounts`); history syncs between the user's devices over Tatakai Link.

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

    // --- parity wave A (15 §3.1, worker o: :core:metadata + core/data/MetadataRepositoryImpl.kt). Defaults = "not available". ---
    /**
     * Search with every AniList filter applied server-side (genres, tags, format, status, season, year / episode / score ranges, sort,
     * 18+, studio). Default: plain [search] with `query.query` (filters ignored). Pagination through [FeedPage.hasNext].
     */
    suspend fun searchAdvanced(kind: MediaKind, query: MediaSearchQuery, page: Int = 1): FeedPage =
        search(kind, query.query.orEmpty(), page)

    /** Technical sheet, trailer and next airing episode of a title; null when the provider has nothing. */
    suspend fun details(ref: MediaRef): MediaDetails? = null

    /** Related anime and manga (AniList `relations`). */
    suspend fun relations(ref: MediaRef): List<MediaRelation> = emptyList()

    /** Seasons / watch order: the chain of PREQUEL / SEQUEL relations from the first season, chronological, [ref] included. */
    suspend fun seasons(ref: MediaRef): List<Media> = emptyList()

    suspend fun characters(ref: MediaRef, page: Int = 1): CharactersPage = CharactersPage(emptyList(), page, false)

    suspend fun staff(ref: MediaRef): List<StaffEdge> = emptyList()

    suspend fun recommendations(ref: MediaRef): List<Recommendation> = emptyList()

    suspend fun character(id: Int): Character? = null

    /** Search type "Personajes" (classic unified search): AniList character search, one page of [PER_PAGE_CHARACTERS]. */
    suspend fun searchCharacters(query: String, page: Int = 1): List<CharacterRef> = emptyList()

    /** The genre list of the provider (search filter chips); default = the 18 hardcoded genres of the current filter sheet. */
    suspend fun genres(kind: MediaKind): List<String> = emptyList()

    /** AniList tag catalogue (name, category, isSpoiler) for the tag filter. */
    suspend fun tags(kind: MediaKind): List<MediaTag> = emptyList()

    /** trace.moe image search (classic `/image-search`): the picked image's bytes; results best first. */
    suspend fun searchByImage(image: ByteArray, mimeType: String): List<ImageMatch> = emptyList()
}

data class MetadataCacheStats(val entries: Int, val bytes: Long)

/** Page size of [MetadataRepository.searchCharacters]. */
const val PER_PAGE_CHARACTERS: Int = 20

/**
 * Local watch history (Room) with the exact resume position of every episode. There is no account behind it (plan 14 §0.1 #9: Next has
 * no own login): the rows are synced between the user's own devices over Tatakai Link (`/v1/history` of the paired PC, mTLS), and the
 * episode COUNT reaches AniList / MAL through [ListsRepository.onEpisodeCompleted]. Without a paired PC everything stays local.
 */
interface HistoryRepository {
    fun continueWatching(limit: Int = 20): Flow<List<HistoryEntry>>
    suspend fun record(entry: HistoryEntry)
    fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>>

    // --- android wave (worker a) ---
    /** Library › Historial: one row per watched episode, newest first (not grouped per series like [continueWatching]). */
    fun history(limit: Int = 200): Flow<List<HistoryEntry>> = continueWatching(limit)

    /**
     * "Sincronizar ahora" (You, Library, Settings › Enlace): push dirty rows to the paired PC and pull its recent ones over Tatakai Link.
     * Success without a paired PC (or with `AppSettings.historySyncOverLink` off) = no-op.
     */
    suspend fun syncNow(): Result<Unit> = Result.success(Unit)

    /** Epoch ms of the last successful Link sync; null = never / no paired PC. */
    val lastSyncedAt: StateFlow<Long?> get() = NO_SYNC

    // --- parity wave A (15 §3.4, worker q) ---
    /** Settings › Datos › Exportar historial: JSON array of every local row (the desktop export format), newest first. */
    suspend fun exportJson(): String = "[]"

    /**
     * Settings › Datos › Borrar historial (with confirmation): local rows. [includeRemote] is kept for desktop parity, but Link v1 has
     * no `DELETE /v1/history`, so in wave A it only resets the Link cursor (the PC's copy is cleared on the PC; KNOWN-ISSUES).
     */
    suspend fun clearHistory(includeRemote: Boolean): Result<Unit> = Result.failure(NotImplementedError("clearHistory"))
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

    // --- parity wave A (15 §3.1, worker o: override in core/extensions-quickjs/ExtensionRepositoryImpl.kt, additive) ---
    /**
     * Search inside one extension (route `search` of the extension host, same shape as `custom/home` items). Tapping a result
     * searches its title in the metadata provider (as the Home extension rows do). Default: nothing.
     */
    suspend fun searchExtension(extensionId: String, query: String): List<ExtensionHomeItem> = emptyList()

    // --- integrated extension server (docs/redesign/13 §3.10) ---
    /**
     * True while a source that is not an installed extension (an enabled, linked PC extension server) takes part in
     * [resolveCandidates], so screens do not show "no source installed" when only the PC provides sources.
     */
    val externalSourcesAvailable: Flow<Boolean> get() = flowOf(false)
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
