// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ChapterDownload
import app.tatakai.next.model.ChapterDownloadSource
import app.tatakai.next.model.ChapterKind
import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import app.tatakai.next.model.MangaDetails
import app.tatakai.next.model.MangaHome
import app.tatakai.next.model.MangaRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.model.ReadingProgress
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import okhttp3.OkHttpClient
import java.io.File

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.5, worker s): [MangaRepository] + the manga [ChapterDownloadSource].
 *
 * Plan for s: home / search / details through [metadata] (`feed(MANGA, …)`, `searchAdvanced(MANGA, …)`, `details`, `relations` for the
 * anime adaptation); MangaUpdates (`api.mangaupdates.com/v1`, series search + `/series/{id}`) and MangaDex (`api.mangadex.org`, only as a
 * chapter list fallback when no `.kai` manga source is installed) as private clients in this module; chapters through
 * `ExtensionRepository.customSources("read")` + `providerSearch` (title match, cached per (ext, source, mediaId) in `storeDir/match/`) +
 * `providerInfo` (entries of kind `read`) + `providerRead` (pages). Progress, readlist and downloaded chapters persist in this module
 * (JSON files or a private Room db `manga.db`, see NOTES-deps.md); the readlist also writes AniList progress with
 * `ListsRepository.accessToken(ANILIST)` (`SaveMediaListEntry(mediaId, status, progress)`) when linked. Downloaded pages go to
 * `storeDir/chapters/<chapterId>/<index>.<ext>` and are exposed as `file://` [ChapterPage]s.
 *
 * Stub: empty home, no sources, every action fails with a clear error.
 */
class MangaRepositoryImpl(
    private val metadata: MetadataRepository,
    private val extensions: ExtensionRepository,
    private val lists: ListsRepository? = null,
    private val client: OkHttpClient,
    private val storeDir: File,
    private val settings: () -> AppSettings = { AppSettings() },
    private val clock: () -> Long = System::currentTimeMillis,
) : MangaRepository, ChapterDownloadSource, AppDataWiper {
    private val progress = MutableStateFlow<Map<String, ReadingProgress>>(emptyMap())
    private val readlist = MutableStateFlow<List<ReadEntry>>(emptyList())
    private val chapterDownloads = MutableStateFlow<List<ChapterDownload>>(emptyList())

    override fun home(): Flow<MangaHome> = flowOf(MangaHome())

    override suspend fun refreshHome() {}

    override suspend fun search(query: MediaSearchQuery, page: Int): FeedPage = metadata.searchAdvanced(MediaKind.MANGA, query, page)

    override suspend fun details(ref: MediaRef): MangaDetails? = null

    override suspend fun chapterSources(ref: MediaRef, title: String): List<MangaChapterSource> = emptyList()

    override suspend fun chapters(source: MangaChapterSource, refresh: Boolean): List<MangaChapter> = emptyList()

    override suspend fun bestChapter(ref: MediaRef, title: String, number: Double): MangaChapter? = null

    override suspend fun pages(chapter: MangaChapter): List<ChapterPage> = emptyList()

    override suspend fun clearChapterCache(ref: MediaRef) {}

    override fun lastRead(limit: Int): Flow<List<ReadingProgress>> =
        progress.map { map -> map.values.sortedByDescending { it.updatedAtMs }.take(limit) }

    override fun progress(mediaId: String): Flow<ReadingProgress?> = progress.map { it[mediaId] }

    override suspend fun saveProgress(progress: ReadingProgress) {
        this.progress.value = this.progress.value + (progress.mediaId to progress)
    }

    override fun readlist(status: ListStatus?): Flow<List<ReadEntry>> =
        readlist.map { list -> if (status == null) list else list.filter { it.status == status } }

    override fun readEntry(ref: MediaRef): Flow<ReadEntry?> = readlist.map { list -> list.firstOrNull { it.ref == ref } }

    override suspend fun setReadStatus(media: Media, status: ListStatus?): Result<Unit> = notReady("setReadStatus")

    override suspend fun setReadProgress(ref: MediaRef, chapters: Int): Result<Unit> = notReady("setReadProgress")

    override suspend fun downloadChapters(media: Media, chapters: List<MangaChapter>): Result<Int> = notReady("downloadChapters")

    override fun downloadedChapters(mediaId: String): Flow<List<MangaChapter>> = flowOf(emptyList())

    override suspend fun removeDownloadedChapter(chapterId: String) {}

    override suspend fun offlinePages(chapterId: String): List<ChapterPage> = emptyList()

    // --- ChapterDownloadSource (Downloads page of r) ---
    override val kind: ChapterKind = ChapterKind.MANGA

    override fun downloads(): Flow<List<ChapterDownload>> = chapterDownloads

    override suspend fun remove(id: String) = removeDownloadedChapter(id)

    override suspend fun retry(id: String) {}

    /** "Restablecer la app": progress, readlist, chapter cache and downloaded pages. */
    override suspend fun wipe() {
        progress.value = emptyMap()
        readlist.value = emptyList()
        chapterDownloads.value = emptyList()
        storeDir.deleteRecursively()
    }

    private fun <T> notReady(what: String): Result<T> = Result.failure(NotImplementedError("wave B worker s: MangaRepositoryImpl.$what"))

    companion object {
        const val DIR_NAME: String = "manga"
        const val MANGAUPDATES_BASE_URL: String = "https://api.mangaupdates.com/v1"
        const val MANGADEX_BASE_URL: String = "https://api.mangadex.org"
    }
}
