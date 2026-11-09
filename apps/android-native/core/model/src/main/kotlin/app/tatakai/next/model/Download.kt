// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.2): web downloads (HLS / MP4) with the Media3
// DownloadManager, queue, "download season", auto-download of new episodes (WorkManager) and the local library. Desktop counterpart:
// tk-core/download/** (worker h). Signatures FINAL; bodies: worker r (:core:download, `downloadModule`). Torrent downloads stay in
// `TorrentRepository` (:core:torrent, finished by q in wave A); the Downloads page of r shows both.

/** Lifecycle of one web download. [REMOVING] = the user removed it and the files are being deleted. */
@Serializable
enum class DownloadState(val id: String) {
    @SerialName("queued") QUEUED("queued"),
    @SerialName("downloading") DOWNLOADING("downloading"),
    @SerialName("paused") PAUSED("paused"),
    @SerialName("completed") COMPLETED("completed"),
    @SerialName("failed") FAILED("failed"),
    @SerialName("removing") REMOVING("removing"),

    /** Checklist downloads.integrity-check: the file of a completed download is missing or truncated ("roto"): re-download or remove. */
    @SerialName("broken") BROKEN("broken"),
}

/** Tabs of the Downloads page (`Route.Downloads`, checklist downloads.page); LOCAL = the local library (plan §7 D6: offline library lives here). */
@Serializable
enum class DownloadsTab(val id: String) {
    @SerialName("active") ACTIVE("active"),
    @SerialName("anime") ANIME("anime"),
    @SerialName("manga") MANGA("manga"),
    @SerialName("novels") NOVELS("novels"),
    @SerialName("history") HISTORY("history"),
    @SerialName("local") LOCAL("local"),
}

/**
 * One episode download (web source). [id] is the Media3 `DownloadRequest.id` (`"<mediaId>?ep=<episode>"`, same shape as
 * `HistoryEntry.episodeId`). [localUri] is set once [state] is [DownloadState.COMPLETED]: a `file://` of the app-specific download
 * directory or a `content://` of the user's SAF folder (`DownloadSettings.directoryTreeUri`), playable through `PlayerArgs.localUri`.
 */
@Serializable
data class DownloadItem(
    val id: String,
    val ref: MediaRef,
    val title: String,
    val poster: String? = null,
    val episode: Int,
    /** Extension that produced the source and its server label (`SourceCandidate.extId` / `label`). */
    val extId: String,
    val sourceLabel: String,
    val container: Container,
    val state: DownloadState,
    /** 0.0..1.0 (HLS: segments done / total; MP4: bytes). */
    val progress: Double = 0.0,
    val downloadedBytes: Long = 0,
    val totalBytes: Long? = null,
    val localUri: String? = null,
    /** Short error code / message when [state] is [DownloadState.FAILED]. */
    val error: String? = null,
    val createdAtMs: Long,
    val completedAtMs: Long? = null,
)

/** What Details / the player / the season sheet hand to [DownloadRepository.enqueue]. */
data class DownloadRequest(
    val media: Media,
    val episode: Int,
    /** A resolved web stream (`SourceCandidate.toStream()` or the sniffed embed capture). Torrents go to `TorrentRepository`. */
    val stream: StreamSource,
    /** `SourceCandidate.label` (server), shown in the Downloads rows. */
    val sourceLabel: String,
)

/** Classic AutomationProfiles: how the auto-downloader picks a source for a new episode. */
@Serializable
enum class DownloadProfile(val id: String) {
    @SerialName("best") BEST("best"),
    @SerialName("balanced_1080") BALANCED_1080("balanced_1080"),
    @SerialName("saver_720") SAVER_720("saver_720"),
}

/** Auto-download rule of one series (classic `automation-engine.ts`): new episodes of a WATCHING title get queued automatically. */
@Serializable
data class AutoDownloadRule(
    val ref: MediaRef,
    val title: String,
    val poster: String? = null,
    val enabled: Boolean = true,
    val profile: DownloadProfile = DownloadProfile.BALANCED_1080,
    /** Highest episode number already queued or downloaded by the rule. */
    val lastEpisode: Int = 0,
    val lastCheckedMs: Long? = null,
)

/** "Simular": what the next check would queue for one rule, without queuing it. */
data class AutoDownloadPreview(val rule: AutoDownloadRule, val episodes: List<Int>, val estimatedBytes: Long?)

/**
 * One video of the local library (classic offline library: imported files + scanned folders). [ref] / [episode] are null until the file
 * is matched (release-name parse of `:core:release`, confirmed or corrected by the user through [DownloadRepository.matchLocal]).
 */
@Serializable
data class LocalLibraryItem(
    val id: String,
    /** `content://` (SAF) or `file://`. */
    val uri: String,
    val fileName: String,
    val sizeBytes: Long,
    val ref: MediaRef? = null,
    val title: String? = null,
    val poster: String? = null,
    val episode: Int? = null,
    val addedAtMs: Long,
) {
    val matched: Boolean get() = ref != null && episode != null
}

/** Settings › Descargas (`AppSettings.downloads`). Web downloads only; torrents keep `TorrentSettings`. */
@Serializable
data class DownloadSettings(
    /** SAF tree of the web-download folder; null = app-specific external files dir (`Android/data/<pkg>/files/downloads`). */
    val directoryTreeUri: String? = null,
    /** 1..5 simultaneous downloads. */
    val maxConcurrent: Int = 2,
    val wifiOnly: Boolean = true,
    /** 0 = unlimited; auto-downloads stop (and Downloads shows it) when the folder reaches this many GiB. */
    val storageLimitGib: Int = 0,
    /** Delete a downloaded episode once its history row is `completed` (classic "free watched episodes"). */
    val autoEvictWatched: Boolean = false,
    /** Run the auto-download check (WorkManager, every 30 min, Wi-Fi + battery not low). */
    val autoCheck: Boolean = true,
    val autoProfile: DownloadProfile = DownloadProfile.BALANCED_1080,
    /** Remembered for new downloads (classic default server / preferred audio). */
    val preferredServer: String? = null,
    val preferredAudioLang: String? = null,
) {
    fun sanitized(): DownloadSettings = copy(
        maxConcurrent = maxConcurrent.coerceIn(1, MAX_CONCURRENT),
        storageLimitGib = storageLimitGib.coerceIn(0, MAX_STORAGE_GIB),
        preferredServer = preferredServer?.trim()?.takeIf { it.isNotEmpty() },
        preferredAudioLang = preferredAudioLang?.trim()?.lowercase()?.takeIf { it.isNotEmpty() },
    )

    companion object {
        const val MAX_CONCURRENT: Int = 5
        const val MAX_STORAGE_GIB: Int = 4096
    }
}

/** Numbers for the You row and the Downloads header (web downloads only; torrents add their own in the screen). */
data class DownloadsSummary(val active: Int = 0, val completed: Int = 0, val failed: Int = 0, val bytes: Long = 0)

/** Which reader a downloaded chapter belongs to. */
enum class ChapterKind { MANGA, NOVEL }

/**
 * One downloaded chapter (manga pages or novel text), listed by the Downloads page (r) and produced by the readers (s). [localRef] is
 * whatever the owning repository needs to open it offline (`MangaReaderArgs` / `NovelReaderArgs` are built by the owner from it).
 */
data class ChapterDownload(
    val id: String,
    val kind: ChapterKind,
    /** `HistoryEntry.animeIdOf`-style id of the title (`"<anilistId>"` / `"mal:<id>"`). */
    val mediaId: String,
    val title: String,
    val poster: String?,
    val chapterLabel: String,
    val chapterNumber: Double?,
    val state: DownloadState,
    val progress: Double = 0.0,
    val bytes: Long = 0,
    val createdAtMs: Long,
    val localRef: String,
)

/**
 * The readers expose their chapter downloads to the Downloads page through this (Koin `getAll<ChapterDownloadSource>()`): s binds one
 * for manga (`:core:manga`) and one for novels (`:core:novel`); r only consumes.
 */
interface ChapterDownloadSource {
    val kind: ChapterKind

    fun downloads(): Flow<List<ChapterDownload>>

    suspend fun remove(id: String)

    suspend fun retry(id: String)
}

/**
 * Web downloads over the Media3 `DownloadManager` (HLS segments through `HlsDownloader`, MP4 through `ProgressiveDownloader`, resume
 * from the last complete segment / byte range, headers of the extension on every request), the queue, the season sheet, the
 * auto-downloader and the local library. Implemented by worker r in :core:download (`DownloadRepositoryImpl`, bound in `downloadModule`).
 */
interface DownloadRepository {
    /** Every known web download, newest first (queue + completed + failed). */
    val items: StateFlow<List<DownloadItem>>
    val summary: StateFlow<DownloadsSummary>

    /** Queues one episode. Fails with [AppException] when the folder limit is reached or the stream is not downloadable (DASH, embed). */
    suspend fun enqueue(request: DownloadRequest): Result<DownloadItem>

    /** "Descargar temporada": one [enqueue] per episode, skipping the ones already downloaded; returns the queued items. */
    suspend fun enqueueSeason(media: Media, episodes: List<Int>, preferredServer: String?, preferredAudioLang: String?): Result<List<DownloadItem>>

    suspend fun pause(id: String)

    suspend fun resume(id: String)

    /** Removes the download and its files. */
    suspend fun remove(id: String)

    suspend fun retry(id: String)

    /** Completed and failed downloads with dates (Downloads › Historial). */
    fun history(limit: Int = 200): Flow<List<DownloadItem>>

    suspend fun clearHistory()

    /** The local file of an episode (completed web download or matched library item); null = play online. */
    suspend fun localUriFor(ref: MediaRef, episode: Int): String?

    /** Completed downloads of one title, by episode (Details marks them and plays offline). */
    fun downloadedEpisodes(ref: MediaRef): Flow<Map<Int, DownloadItem>>

    // --- auto-download (classic AutomationProfiles) ---
    val rules: StateFlow<List<AutoDownloadRule>>

    suspend fun setRule(rule: AutoDownloadRule?)

    suspend fun preview(ref: MediaRef): AutoDownloadPreview?

    /** "Comprobar ahora": runs the check immediately; returns how many episodes were queued. */
    suspend fun checkNow(): Result<Int>

    // --- local library (import, scan a folder, match, play offline) ---
    val library: StateFlow<List<LocalLibraryItem>>

    /** Import picked videos (SAF `OpenMultipleDocuments`): parses the names and proposes title + episode. */
    suspend fun importFiles(uris: List<String>): Result<List<LocalLibraryItem>>

    /** Scans a SAF tree and adds every video found (idempotent on the uri). */
    suspend fun scanFolder(treeUri: String): Result<List<LocalLibraryItem>>

    /** Assigns (or clears) the title and episode of a library item. */
    suspend fun matchLocal(id: String, media: Media?, episode: Int?)

    suspend fun removeLocal(id: String, deleteFile: Boolean)

    /** Bytes used by the web-download folder (Settings › Descargas, storage limit banner). */
    suspend fun storageUsedBytes(): Long

    /**
     * Checklist downloads.integrity-check: checks every COMPLETED item (file exists, size ≥ `totalBytes` when known, HLS output parses);
     * marks the bad ones [DownloadState.BROKEN] and returns them. Runs at app start (r) and from Downloads › "Comprobar".
     */
    suspend fun verifyIntegrity(): List<DownloadItem>
}
