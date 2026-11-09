// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ChapterDownload
import app.tatakai.next.model.ChapterDownloadSource
import app.tatakai.next.model.ChapterKind
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.NovelChapter
import app.tatakai.next.model.NovelChapterText
import app.tatakai.next.model.NovelHome
import app.tatakai.next.model.NovelProgress
import app.tatakai.next.model.NovelRepository
import app.tatakai.next.model.NovelSourceSummary
import app.tatakai.next.model.ReadEntry
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.flow.map
import okhttp3.OkHttpClient
import java.io.File

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.6, worker s): [NovelRepository] + the novel [ChapterDownloadSource].
 * Titles = AniList MANGA with `format == "NOVEL"` (`searchAdvanced(MANGA, query.copy(formats = listOf("NOVEL")))`; home feeds filtered the
 * same way). Chapters through `ExtensionRepository.customSources("novel")` → `providerSearch` / `providerInfo` / `providerRead` (paragraphs
 * = `ChapterPage.url` strings when the source returns text, or HTML stripped with jsoup). Progress / readlist / downloaded text persist in
 * this module (`storeDir/*.json` or a private SQLite `novel.db`); AniList progress through `ListsRepository.accessToken(ANILIST)` when linked.
 *
 * Stub: empty home, no sources, every action fails with a clear error.
 */
class NovelRepositoryImpl(
    private val metadata: MetadataRepository,
    private val extensions: ExtensionRepository,
    private val lists: ListsRepository? = null,
    private val client: OkHttpClient,
    private val storeDir: File,
    private val settings: () -> AppSettings = { AppSettings() },
    private val clock: () -> Long = System::currentTimeMillis,
) : NovelRepository, ChapterDownloadSource, AppDataWiper {
    private val progress = MutableStateFlow<Map<String, NovelProgress>>(emptyMap())
    private val readlist = MutableStateFlow<List<ReadEntry>>(emptyList())
    private val chapterDownloads = MutableStateFlow<List<ChapterDownload>>(emptyList())

    override fun home(): Flow<NovelHome> = flowOf(NovelHome())

    override suspend fun refreshHome() {}

    override suspend fun search(query: MediaSearchQuery, page: Int): FeedPage =
        metadata.searchAdvanced(MediaKind.MANGA, query.copy(formats = listOf(NOVEL_FORMAT)), page)

    override suspend fun sources(ref: MediaRef, title: String): List<NovelSourceSummary> = emptyList()

    override suspend fun checkSources(ref: MediaRef, title: String): List<NovelSourceSummary> = emptyList()

    override suspend fun chapters(source: NovelSourceSummary, refresh: Boolean): List<NovelChapter> = emptyList()

    override suspend fun text(chapter: NovelChapter): NovelChapterText = NovelChapterText(chapter.id, chapter.title, emptyList())

    override fun lastRead(limit: Int): Flow<List<NovelProgress>> =
        progress.map { map -> map.values.sortedByDescending { it.updatedAtMs }.take(limit) }

    override fun progress(mediaId: String): Flow<NovelProgress?> = progress.map { it[mediaId] }

    override suspend fun saveProgress(progress: NovelProgress) {
        this.progress.value = this.progress.value + (progress.mediaId to progress)
    }

    override fun readlist(status: ListStatus?): Flow<List<ReadEntry>> =
        readlist.map { list -> if (status == null) list else list.filter { it.status == status } }

    override fun readEntry(ref: MediaRef): Flow<ReadEntry?> = readlist.map { list -> list.firstOrNull { it.ref == ref } }

    override suspend fun setReadStatus(media: Media, status: ListStatus?): Result<Unit> = notReady("setReadStatus")

    override suspend fun setReadProgress(ref: MediaRef, chapters: Int): Result<Unit> = notReady("setReadProgress")

    override suspend fun downloadChapters(media: Media, chapters: List<NovelChapter>): Result<Int> = notReady("downloadChapters")

    override fun downloadedChapters(mediaId: String): Flow<List<NovelChapter>> = flowOf(emptyList())

    override suspend fun removeDownloadedChapter(chapterId: String) {}

    override suspend fun offlineText(chapterId: String): NovelChapterText? = null

    // --- ChapterDownloadSource (Downloads page of r) ---
    override val kind: ChapterKind = ChapterKind.NOVEL

    override fun downloads(): Flow<List<ChapterDownload>> = chapterDownloads

    override suspend fun remove(id: String) = removeDownloadedChapter(id)

    override suspend fun retry(id: String) {}

    override suspend fun wipe() {
        progress.value = emptyMap()
        readlist.value = emptyList()
        chapterDownloads.value = emptyList()
        storeDir.deleteRecursively()
    }

    private fun <T> notReady(what: String): Result<T> = Result.failure(NotImplementedError("wave B worker s: NovelRepositoryImpl.$what"))

    companion object {
        const val DIR_NAME: String = "novels"

        /** AniList `MediaFormat` of light novels (`MediaDetails.format`). */
        const val NOVEL_FORMAT: String = "NOVEL"
    }
}
