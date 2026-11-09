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
// 2026-11-09 amendment (plan 14 §0.1 #9): the hosted-backend `AuthRepository` / `UserSession` are gone. Identity = AniList and/or MAL
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

    // --- parity wave B (16 §3.1, worker r: AniList `AiringSchedule` in :core:metadata + core/data/MetadataRepositoryImpl.kt). Default = "not available". ---
    /**
     * Episodes airing in `[fromEpochSec, toEpochSec)`, soonest first, one page of `PER_PAGE_AIRING` (AniList `Page.airingSchedules`).
     * Used by the calendar (r, `AiringRepository`) and by the "Últimos episodios" row (last 7 days, newest first).
     */
    suspend fun airingSchedule(fromEpochSec: Long, toEpochSec: Long, page: Int = 1): List<AiringEpisode> = emptyList()

    /**
     * Checklist settings.metadata.provider-status (r): one row per provider of `AppSettings.KNOWN_PROVIDER_IDS` with its last result and
     * cooldown (`MetadataRouter.isCoolingDown`). Default = nothing known (Settings shows the providers without status).
     */
    fun providerStatus(): Flow<List<ProviderStatus>> = emptyFlow()
}

/** Page size of [MetadataRepository.airingSchedule]. */
const val PER_PAGE_AIRING: Int = 50

/** Last outcome of a metadata provider for Settings › Metadatos (checklist settings.metadata.provider-status). */
data class ProviderStatus(
    val id: String,
    val enabled: Boolean,
    /** null = never called in this process. */
    val lastOkAtMs: Long? = null,
    val lastError: String? = null,
    /** Epoch ms until which the router skips the provider ("en pausa N min"); null = not cooling down. */
    val cooldownUntilMs: Long? = null,
)

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

    // --- parity wave B (16 §3.4, worker t: overrides in core/extensions-quickjs/ExtensionRepositoryImpl.kt). Defaults = "not available". ---
    /** Install from a URL of a `.kai` package or of a `manifest.json` next to one (checklist `ext.install-url`). */
    suspend fun installFromUrl(url: String): Result<InstalledExtension> = Result.failure(NotImplementedError("installFromUrl"))

    /** Newer versions known for the installed extensions (filled by [checkUpdates], at app start when `AppSettings.extensionUpdateCheck`). */
    val updates: StateFlow<List<ExtensionUpdate>> get() = NO_UPDATES

    /** Compares every installed extension with the repositories that list it; returns and publishes the updates. */
    suspend fun checkUpdates(): List<ExtensionUpdate> = emptyList()

    /** Downloads and installs the update of one extension (keeps its enabled flag and preferences). */
    suspend fun update(extensionId: String): Result<InstalledExtension> = Result.failure(NotImplementedError("update"))

    /** Manifest `preferences[]` of an installed extension; empty = no form. */
    suspend fun preferences(extensionId: String): List<ExtensionPreference> = emptyList()

    fun preferenceValues(extensionId: String): Flow<Map<String, String>> = emptyFlow()

    /** null removes the value (back to the default). */
    suspend fun setPreference(extensionId: String, key: String, value: String?) {}

    val repositories: StateFlow<List<ExtensionRepo>> get() = NO_REPOS

    /** Fetches the index of [url] and stores the repository (checklist `ext.repositories`). */
    suspend fun addRepository(url: String): Result<ExtensionRepo> = Result.failure(NotImplementedError("addRepository"))

    suspend fun removeRepository(url: String) {}

    suspend fun refreshRepository(url: String): Result<ExtensionRepo> = Result.failure(NotImplementedError("refreshRepository"))

    /** Custom sources of the enabled extensions; [kind] filters `watch` | `read` | `novel` (null = all). Consumed by r (providers), s (readers). */
    suspend fun customSources(kind: String? = null): List<CustomSourceRef> = emptyList()

    /** `custom/home` of ONE source (provider page rows). */
    suspend fun providerHome(extensionId: String, sourceId: String): List<ExtensionHomeRow> = emptyList()

    /** `custom/search` of ONE source. */
    suspend fun providerSearch(extensionId: String, sourceId: String, query: String, page: Int = 1): List<ExtensionHomeItem> = emptyList()

    /** `custom/info`: the title page with its episodes / chapters. */
    suspend fun providerInfo(extensionId: String, sourceId: String, itemId: String): ProviderItem? = null

    /** `custom/watch`: streams of one `watch` entry (played by the normal player, history under [ProviderItem.match] when present). */
    suspend fun providerWatch(extensionId: String, sourceId: String, itemId: String, entryId: String): List<StreamSource> = emptyList()

    /** `custom/read`: pages of one `read` entry (manga reader of s) or paragraphs of a `novel` entry (text joined by the caller). */
    suspend fun providerRead(extensionId: String, sourceId: String, itemId: String, entryId: String): List<ChapterPage> = emptyList()

    fun providerBookmarks(): Flow<List<ProviderBookmark>> = emptyFlow()

    /** Adds / replaces a bookmark; [bookmark] with `completed = true` marks it done. */
    suspend fun setProviderBookmark(bookmark: ProviderBookmark) {}

    suspend fun removeProviderBookmark(extensionId: String, sourceId: String, itemId: String) {}

    /**
     * Checklist ext.kill-switch: "Desactivar todas" / "Activar todas" flips `enabled` of every installed extension without uninstalling
     * (state files only; preferences kept). Implemented by r in ExtensionRepositoryImpl.
     */
    suspend fun setAllEnabled(enabled: Boolean) {}
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

    // --- parity wave B (16 §3.11, worker r; checklist link.remote-control / link.handoff / link.discovery). Defaults = "not available",
    // the Linked screen keeps its buttons disabled while `GET /v1/player` answers 404. Canonical wire in §3.11 (= desktop tk-link/src/control.rs). ---

    /** `GET /v1/player`: what the PC is playing; null = nothing open or no remote control on the PC (404). Polled every 2 s while the Linked screen is visible. */
    suspend fun remoteState(): Result<RemotePlayerState?> = Result.success(null)

    /** `POST /v1/control` `{ "command": "<RemoteAction.id>" }`. */
    suspend fun remote(action: RemoteAction): Result<Unit> = Result.failure(NotImplementedError("remote"))

    /**
     * "Continuar en la PC" (`POST /v1/control` `{ "command": "open", "mediaId": animeId, "episode", "positionSecs": positionSec }`): the PC opens the same episode at the phone's position.
     * The phone pushes its history first (`HistoryRepository.syncNow`) so the PC also has the row.
     */
    suspend fun handoff(animeId: String, episode: Int, positionSec: Int): Result<Unit> = Result.failure(NotImplementedError("handoff"))
}

/** Transport controls of the PC player over Link (desktop k serves them; Android r sends them). */
enum class RemoteAction(val id: String) { PREV("prev"), PLAY_PAUSE("toggle"), NEXT("next") }

/** `GET /v1/player` of the PC (null fields = the PC does not know). Wire keys: positionSecs, durationSecs, mediaId (mapped by LinkClient). */
data class RemotePlayerState(
    val playing: Boolean,
    val title: String? = null,
    val episode: Int? = null,
    val positionSec: Int = 0,
    val durationSec: Int? = null,
    /** `HistoryEntry.animeId` of what is playing, when the PC knows it. */
    val animeId: String? = null,
)

private val NO_SYNC: StateFlow<Long?> = MutableStateFlow<Long?>(null).asStateFlow()
private val NO_LINKED_DEVICE: StateFlow<LinkedDevice?> = MutableStateFlow<LinkedDevice?>(null).asStateFlow()
private val NO_UPDATES: StateFlow<List<ExtensionUpdate>> = MutableStateFlow<List<ExtensionUpdate>>(emptyList()).asStateFlow()
private val NO_REPOS: StateFlow<List<ExtensionRepo>> = MutableStateFlow<List<ExtensionRepo>>(emptyList()).asStateFlow()
