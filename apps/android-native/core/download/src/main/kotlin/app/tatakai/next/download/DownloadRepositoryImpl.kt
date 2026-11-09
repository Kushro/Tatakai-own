// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import android.content.Context
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AutoDownloadPreview
import app.tatakai.next.model.AutoDownloadRule
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.DownloadRequest
import app.tatakai.next.model.DownloadSettings
import app.tatakai.next.model.DownloadsSummary
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LocalLibraryItem
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.NotificationExtras
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import java.io.File

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r): [DownloadRepository] over the Media3
 * `DownloadManager` (`DefaultDownloadIndex` on a `StandaloneDatabaseProvider` of `downloads.db`, `SimpleCache` in the download folder,
 * `DefaultHttpDataSource` / OkHttp factory carrying the extension headers of each request through `DownloadRequest.data` = JSON of
 * [DownloadItem] + headers). HLS = `HlsDownloader` (all segments, resumes from the index), MP4 = `ProgressiveDownloader` (Range). The
 * service [TatakaiDownloadService] keeps the manager alive in the foreground. Playback of a completed download: `CacheDataSource` is
 * NOT used; r remuxes / copies the cached bytes into one playable file (`<folder>/<mediaId>/<episode>.<mp4|ts>`) when the download
 * completes, so `PlayerArgs.localUri` and the external player work offline.
 *
 * Private stores (JSON under `filesDir/downloads/`): `library.json` ([LocalLibraryItem]), `rules.json` ([AutoDownloadRule]),
 * `history.json` (finished / failed items beyond the index). [settings] gives the folder, concurrency, Wi-Fi rule and limits.
 *
 * Stub: every flow is empty and every action fails with a clear error until r implements it.
 */
class DownloadRepositoryImpl(
    private val context: Context,
    private val settings: () -> DownloadSettings,
    private val extensions: ExtensionRepository,
    private val metadata: MetadataRepository,
    private val history: HistoryRepository,
    /** Auto-download of WATCHING titles (`AppSettings.downloads.autoCheck`); null = lists not bound. */
    private val lists: ListsRepository? = null,
    private val storeDir: File = File(context.filesDir, DIR_NAME),
    private val clock: () -> Long = System::currentTimeMillis,
) : DownloadRepository, AppDataWiper {
    private val _items = MutableStateFlow<List<DownloadItem>>(emptyList())
    override val items: StateFlow<List<DownloadItem>> = _items.asStateFlow()
    private val _summary = MutableStateFlow(DownloadsSummary())
    override val summary: StateFlow<DownloadsSummary> = _summary.asStateFlow()
    private val _rules = MutableStateFlow<List<AutoDownloadRule>>(emptyList())
    override val rules: StateFlow<List<AutoDownloadRule>> = _rules.asStateFlow()
    private val _library = MutableStateFlow<List<LocalLibraryItem>>(emptyList())
    override val library: StateFlow<List<LocalLibraryItem>> = _library.asStateFlow()

    override suspend fun enqueue(request: DownloadRequest): Result<DownloadItem> = notReady("enqueue")

    override suspend fun enqueueSeason(media: Media, episodes: List<Int>, preferredServer: String?, preferredAudioLang: String?): Result<List<DownloadItem>> =
        notReady("enqueueSeason")

    override suspend fun pause(id: String) {}

    override suspend fun resume(id: String) {}

    override suspend fun remove(id: String) {}

    override suspend fun retry(id: String) {}

    override fun history(limit: Int): Flow<List<DownloadItem>> = flowOf(emptyList())

    override suspend fun clearHistory() {}

    override suspend fun localUriFor(ref: MediaRef, episode: Int): String? = null

    override fun downloadedEpisodes(ref: MediaRef): Flow<Map<Int, DownloadItem>> = _items.map { list ->
        list.filter { it.ref == ref && it.localUri != null }.associateBy { it.episode }
    }

    override suspend fun setRule(rule: AutoDownloadRule?) {}

    override suspend fun preview(ref: MediaRef): AutoDownloadPreview? = null

    override suspend fun checkNow(): Result<Int> = notReady("checkNow")

    override suspend fun importFiles(uris: List<String>): Result<List<LocalLibraryItem>> = notReady("importFiles")

    override suspend fun scanFolder(treeUri: String): Result<List<LocalLibraryItem>> = notReady("scanFolder")

    override suspend fun matchLocal(id: String, media: Media?, episode: Int?) {}

    override suspend fun removeLocal(id: String, deleteFile: Boolean) {}

    override suspend fun storageUsedBytes(): Long = 0

    override suspend fun verifyIntegrity(): List<DownloadItem> = emptyList()

    /** "Restablecer la app": removes every download, the index, the library and the rules. */
    override suspend fun wipe() {
        _items.value = emptyList()
        _rules.value = emptyList()
        _library.value = emptyList()
        storeDir.deleteRecursively()
        context.getDatabasePath(DB_NAME).let { db -> db.delete(); File(db.path + "-journal").delete() }
    }

    private fun <T> notReady(what: String): Result<T> = Result.failure(NotImplementedError("wave B worker r: DownloadRepositoryImpl.$what"))

    companion object {
        const val DIR_NAME: String = "downloads"
        const val DB_NAME: String = "downloads.db"

        /** Notification channel shared with [TatakaiDownloadService]. */
        const val CHANNEL_ID: String = "downloads"

        /** Extra of the "download finished" notification tap (read by `MainActivity` through `WaveBLinks`): `DownloadsTab.id`. */
        const val EXTRA_TAB: String = NotificationExtras.DOWNLOADS_TAB
    }
}
