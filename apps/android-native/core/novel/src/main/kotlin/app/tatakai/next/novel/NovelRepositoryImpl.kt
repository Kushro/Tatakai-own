// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import app.tatakai.next.manga.AniListMangaApi
import app.tatakai.next.manga.ChapterText
import app.tatakai.next.manga.JsonFileStore
import app.tatakai.next.manga.MangaInfo
import app.tatakai.next.manga.ReaderJson
import app.tatakai.next.manga.ReadlistStore
import app.tatakai.next.manga.TitleMatch
import app.tatakai.next.manga.safeFileName
import app.tatakai.next.manga.sourceKey
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ChapterDownload
import app.tatakai.next.model.ChapterDownloadSource
import app.tatakai.next.model.ChapterKind
import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.NovelChapter
import app.tatakai.next.model.NovelChapterText
import app.tatakai.next.model.NovelHome
import app.tatakai.next.model.NovelProgress
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.NovelRepository
import app.tatakai.next.model.NovelSourceSummary
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.model.ReadListPuller
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import okhttp3.OkHttpClient
import okhttp3.Request
import java.io.File
import java.io.IOException
import java.util.Locale

@Serializable
internal data class NovelMatch(val itemId: String? = null, val atMs: Long = 0)

@Serializable
internal data class CachedNovelChapters(val chapters: List<NovelChapter> = emptyList(), val atMs: Long = 0)

/** A chapter queued / stored in `novel.db` (index in `downloads.json`). */
@Serializable
internal data class DownloadedNovelChapter(
    val id: String,
    val chapter: NovelChapter,
    val mediaId: String,
    val title: String,
    val poster: String? = null,
    val anilistId: Int? = null,
    val state: DownloadState = DownloadState.QUEUED,
    val bytes: Long = 0,
    val createdAtMs: Long,
    val error: String? = null,
)

@Serializable
internal data class NovelUpdate(val entry: ReadEntry, val chapter: NovelChapter, val foundAtMs: Long)

@Serializable
internal data class NovelUpdatesDoc(val seen: Map<String, Double> = emptyMap(), val updates: List<NovelUpdate> = emptyList())

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.6, worker s): [NovelRepository] + the novel [ChapterDownloadSource].
 * Titles = AniList MANGA with `format == "NOVEL"` (private [AniListMangaApi] of :core:manga with `novels = true`). Chapters through
 * `ExtensionRepository.customSources("novel")` → `providerSearch` (title match cached in `matches.json`) / `providerInfo` (entries of kind
 * `novel`, cached 6 h) / `providerRead` (text, HTML stripped with jsoup by [NovelText]). Progress (`progress.json`), readlist
 * ([ReadlistStore] with `novels = true`, AniList `SaveMediaListEntry` when linked), followed updates (`updates.json`), downloaded text in a
 * private SQLite file ([NovelTextStore], `novel.db`) with its index in `downloads.json`.
 */
class NovelRepositoryImpl(
    private val metadata: MetadataRepository,
    private val extensions: ExtensionRepository,
    private val lists: ListsRepository? = null,
    private val client: OkHttpClient,
    private val storeDir: File,
    private val settings: () -> AppSettings = { AppSettings() },
    private val clock: () -> Long = System::currentTimeMillis,
) : NovelRepository, ChapterDownloadSource, AppDataWiper, ReadListPuller {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val anilist = AniListMangaApi(client)

    private val progressStore = JsonFileStore(File(storeDir, "progress.json"), MapSerializer(String.serializer(), NovelProgress.serializer())) { emptyMap() }
    private val matchStore = JsonFileStore(File(storeDir, "matches.json"), MapSerializer(String.serializer(), NovelMatch.serializer())) { emptyMap() }
    private val countStore = JsonFileStore(File(storeDir, "counts.json"), MapSerializer(String.serializer(), Int.serializer())) { emptyMap() }
    private val downloadsStore = JsonFileStore(File(storeDir, "downloads.json"), ListSerializer(DownloadedNovelChapter.serializer())) { emptyList() }
    private val updatesStore = JsonFileStore(File(storeDir, "updates.json"), NovelUpdatesDoc.serializer()) { NovelUpdatesDoc() }
    private val readlistStore = ReadlistStore(
        File(storeDir, "readlist.json"), anilist, lists, novels = true, clock = clock, writeBack = { settings().listsWriteBack },
    )
    private val texts = NovelTextStore(File(storeDir, NovelTextStore.FILE_NAME))

    private val progressState = MutableStateFlow<Map<String, NovelProgress>>(emptyMap())
    private val downloadsState = MutableStateFlow<List<DownloadedNovelChapter>>(emptyList())
    private val updatesState = MutableStateFlow<List<NovelUpdate>>(emptyList())
    private val feeds = MutableStateFlow(NovelHome())
    private val infoCache = HashMap<String, MangaInfo>()
    private val chapterMemo = HashMap<String, CachedNovelChapters>()
    private val memoLock = Mutex()
    private var genresCache: List<String>? = null
    private var downloadJob: Job? = null
    private val downloadLock = Mutex()

    init {
        scope.launch {
            progressState.value = progressStore.read()
            downloadsState.value = downloadsStore.read().map { if (it.state == DownloadState.DOWNLOADING) it.copy(state = DownloadState.QUEUED) else it }
            updatesState.value = updatesStore.read().updates
            readlistStore.load()
            if (downloadsState.value.any { it.state == DownloadState.QUEUED }) startDownloads()
        }
    }

    // ---------------------------------------------------------------- home / search

    override fun home(): Flow<NovelHome> = combine(feeds, lastRead(20), updatesState) { home, last, updates ->
        home.copy(lastRead = last, followedUpdates = updates.map { it.entry to it.chapter })
    }

    override suspend fun refreshHome() {
        feeds.update { it.copy(hero = LoadState.Loading, trending = LoadState.Loading, popular = LoadState.Loading) }
        coroutineScope {
            val trending = async { load { anilist.search(MediaSearchQuery(sort = MediaSort.TRENDING), 1, novels = true).items } }
            val popular = async { load { anilist.search(MediaSearchQuery(sort = MediaSort.POPULARITY), 1, novels = true).items } }
            val genres = async { runCatching { genres() }.getOrDefault(emptyList()) }
            val t = trending.await()
            val hero = if (t is LoadState.Ok) LoadState.Ok(t.value.sortedByDescending { it.banner != null }.take(HERO_SIZE)) else t
            feeds.update { it.copy(hero = hero, trending = t, popular = popular.await(), genres = genres.await()) }
        }
        scope.launch { runCatching { checkFollowedUpdates() } }
    }

    private suspend fun genres(): List<String> {
        genresCache?.let { return it }
        val list = anilist.genres().filterNot { it.equals("Hentai", ignoreCase = true) && !settings().showAdult }
        genresCache = list
        return list
    }

    override suspend fun search(query: MediaSearchQuery, page: Int): FeedPage =
        anilist.search(query.copy(formats = listOf(NOVEL_FORMAT), adult = query.adult && settings().showAdult), page, novels = true)

    private suspend fun info(ref: MediaRef): MangaInfo? {
        val key = mediaKey(ref) ?: return null
        memoLock.withLock { infoCache[key] }?.let { return it }
        val info = anilist.info(ref) ?: return null
        memoLock.withLock { infoCache[key] = info }
        return info
    }

    // ---------------------------------------------------------------- sources / chapters

    override suspend fun sources(ref: MediaRef, title: String): List<NovelSourceSummary> {
        val key = mediaKey(ref) ?: return emptyList()
        val titles = titlesOf(ref, title)
        val custom = runCatching { extensions.customSources("novel") }.getOrDefault(emptyList())
        val counts = countStore.read()
        return coroutineScope {
            custom.map { src ->
                async {
                    val sk = sourceKey(src.extensionId, src.sourceId)
                    val itemId = withTimeoutOrNull(SOURCE_TIMEOUT_MS) {
                        matchFor("$key|$sk") {
                            val hits = titles.take(2).flatMap { t -> runCatching { extensions.providerSearch(src.extensionId, src.sourceId, t) }.getOrDefault(emptyList()) }
                            TitleMatch.best(hits, titles)?.id
                        }
                    }
                    val cached = itemId?.let { cachedChapters(sk, it) }
                    NovelSourceSummary(
                        extensionId = src.extensionId,
                        extensionName = src.extensionName.ifBlank { src.name },
                        sourceId = src.sourceId,
                        itemId = itemId,
                        languages = cached?.let(NovelChapters::languages).orEmpty(),
                        chapterCount = cached?.size ?: itemId?.let { counts["$sk|$it"] },
                        translator = cached?.let(NovelChapters::translators)?.singleOrNull(),
                    )
                }
            }.awaitAll()
        }.sortedWith(compareByDescending<NovelSourceSummary> { it.itemId != null }.thenByDescending { it.chapterCount ?: -1 })
    }

    override suspend fun checkSources(ref: MediaRef, title: String): List<NovelSourceSummary> {
        val found = sources(ref, title)
        return coroutineScope {
            found.map { s ->
                async {
                    if (s.itemId == null) return@async s.copy(chapterCount = 0)
                    val list = withTimeoutOrNull(SOURCE_TIMEOUT_MS) { runCatching { chapters(s, refresh = true) }.getOrNull() }
                    if (list == null) s else s.copy(chapterCount = list.size, languages = NovelChapters.languages(list), translator = NovelChapters.translators(list).singleOrNull())
                }
            }.awaitAll()
        }.sortedWith(compareByDescending<NovelSourceSummary> { it.itemId != null }.thenByDescending { it.chapterCount ?: -1 })
    }

    override suspend fun chapters(source: NovelSourceSummary, refresh: Boolean): List<NovelChapter> {
        val itemId = source.itemId ?: return emptyList()
        val sk = sourceKey(source.extensionId, source.sourceId)
        if (!refresh) cachedChapters(sk, itemId)?.let { return markDownloaded(it) }
        val info = extensions.providerInfo(source.extensionId, source.sourceId, itemId)
            ?: throw AppException(AppError.Extension(source.extensionId, "custom/info: nothing"))
        val list = info.entries
            .filter { it.kind.isBlank() || it.kind == "novel" || it.kind == "read" }
            .map { e ->
                NovelChapter(
                    id = e.id,
                    extensionId = source.extensionId,
                    sourceId = source.sourceId,
                    itemId = itemId,
                    number = e.number ?: ChapterText.number(e.title),
                    title = e.title,
                    lang = e.lang?.lowercase(Locale.ROOT),
                    translator = ChapterText.group(e.title),
                    releasedAtMs = e.releasedAtMs,
                )
            }
        val cached = CachedNovelChapters(list, clock())
        memoLock.withLock { chapterMemo["$sk|$itemId"] = cached }
        countStore.update { it + ("$sk|$itemId" to list.size) }
        runCatching {
            val file = chapterFile(sk, itemId)
            file.parentFile?.mkdirs()
            file.writeText(ReaderJson.encodeToString(CachedNovelChapters.serializer(), cached))
        }
        return markDownloaded(list)
    }

    /** The chapter list of a reader chapter's source (next / previous, continuous scroll). */
    suspend fun chaptersOf(extensionId: String, sourceId: String, itemId: String): List<NovelChapter> {
        val sk = sourceKey(extensionId, sourceId)
        cachedChapters(sk, itemId)?.let { return markDownloaded(it) }
        val offline = downloadsState.value.filter { it.chapter.extensionId == extensionId && it.chapter.sourceId == sourceId && it.chapter.itemId == itemId && it.state == DownloadState.COMPLETED }
        return try {
            chapters(NovelSourceSummary(extensionId, extensionId, sourceId, itemId, emptyList(), null), refresh = true)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            if (offline.isNotEmpty()) offline.map { it.chapter.copy(downloaded = true) } else throw e
        }
    }

    override suspend fun text(chapter: NovelChapter): NovelChapterText {
        offlineText(downloadId(chapter))?.let { return it }
        val pages = try {
            extensions.providerRead(chapter.extensionId, chapter.sourceId, chapter.itemId, chapter.id)
        } catch (e: CancellationException) {
            throw e
        } catch (e: AppException) {
            throw e
        } catch (e: Exception) {
            throw AppException(AppError.Extension(chapter.extensionId, e.message ?: e::class.java.simpleName), e)
        }
        val paragraphs = NovelText.fromPages(pages, ::fetchHtml)
        if (paragraphs.isEmpty()) throw AppException(AppError.Extension(chapter.extensionId, "custom/read: no text"))
        return NovelChapterText(chapter.id, chapter.title, paragraphs)
    }

    private suspend fun fetchHtml(page: ChapterPage): String = withContext(Dispatchers.IO) {
        val request = Request.Builder().url(page.url).apply { page.headers.forEach { (k, v) -> header(k, v) } }.get().build()
        try {
            client.newCall(request).execute().use { r ->
                if (!r.isSuccessful) throw AppException(AppError.Http(r.code))
                r.body.string()
            }
        } catch (e: IOException) {
            throw AppException(AppError.Offline, e)
        }
    }

    // ---------------------------------------------------------------- progress / readlist

    override fun lastRead(limit: Int): Flow<List<NovelProgress>> =
        progressState.map { map -> map.values.sortedByDescending { it.updatedAtMs }.take(limit) }

    override fun progress(mediaId: String): Flow<NovelProgress?> = progressState.map { it[mediaId] }

    override suspend fun saveProgress(progress: NovelProgress) {
        progressState.value = progressStore.update { it + (progress.mediaId to progress) }
        val number = progress.chapterNumber ?: return
        updatesState.value = updatesStore.update { doc ->
            val seen = maxOf(doc.seen[progress.mediaId] ?: 0.0, number)
            doc.copy(seen = doc.seen + (progress.mediaId to seen), updates = doc.updates.filterNot { mediaKey(it.entry.ref) == progress.mediaId && (it.chapter.number ?: 0.0) <= seen })
        }.updates
        if (progress.offset >= COMPLETED_OFFSET) {
            val ref = refOf(progress.mediaId) ?: return
            val chapters = number.toInt()
            val current = readlistStore.current(ref)
            if (current == null || current.progress < chapters) readlistStore.setProgress(ref, chapters, progress.title, progress.poster)
        }
    }

    override fun readlist(status: ListStatus?): Flow<List<ReadEntry>> = readlistStore.flow(status)

    override fun readEntry(ref: MediaRef): Flow<ReadEntry?> = readlistStore.entry(ref)

    override suspend fun setReadStatus(media: Media, status: ListStatus?): Result<Unit> = readlistStore.setStatus(media, status)

    override suspend fun setReadProgress(ref: MediaRef, chapters: Int): Result<Unit> = readlistStore.setProgress(ref, chapters)

    /** Pulls the AniList novel list into the local readlist (Library › Novelas refresh). */
    suspend fun pullReadlist(): Result<Int> = readlistStore.pull()

    /** "Sincronizar ahora" of the lists (ListsRepository.syncNow) refreshes the novel read list too. */
    override suspend fun pullReadList(): Result<Int> = readlistStore.pull()

    suspend fun dismissUpdate(mediaId: String) {
        updatesState.value = updatesStore.update { doc -> doc.copy(updates = doc.updates.filterNot { mediaKey(it.entry.ref) == mediaId }) }.updates
    }

    private suspend fun checkFollowedUpdates() {
        val reading = readlistStore.entries.value.filter { it.status == ListStatus.WATCHING || it.status == ListStatus.REPEATING }.take(MAX_FOLLOWED)
        if (reading.isEmpty()) return
        val now = clock()
        val found = mutableListOf<NovelUpdate>()
        val latest = mutableMapOf<String, Double>()
        reading.forEach { entry ->
            val key = mediaKey(entry.ref) ?: return@forEach
            val source = runCatching { sources(entry.ref, entry.title).firstOrNull { it.itemId != null } }.getOrNull() ?: return@forEach
            val list = runCatching { chapters(source, refresh = true) }.getOrNull() ?: return@forEach
            val top = NovelChapters.sorted(list, ascending = false).firstOrNull { it.number != null } ?: return@forEach
            latest[key] = top.number ?: return@forEach
            found += NovelUpdate(entry, top, now)
        }
        updatesState.value = updatesStore.update { doc ->
            val fresh = found.filter { u ->
                val key = mediaKey(u.entry.ref) ?: return@filter false
                val seen = doc.seen[key] ?: maxOf(progressState.value[key]?.chapterNumber ?: 0.0, u.entry.progress.toDouble())
                (u.chapter.number ?: 0.0) > seen
            }
            val kept = doc.updates.filter { now - it.foundAtMs < UPDATE_TTL_MS && fresh.none { f -> mediaKey(f.entry.ref) == mediaKey(it.entry.ref) } }
            doc.copy(seen = doc.seen + latest.filterKeys { it !in doc.seen }, updates = (fresh + kept).sortedByDescending { it.foundAtMs })
        }.updates
    }

    // ---------------------------------------------------------------- downloads

    override suspend fun downloadChapters(media: Media, chapters: List<NovelChapter>): Result<Int> {
        val mediaId = mediaKey(media.ref) ?: return Result.failure(AppException(AppError.Unknown("no AniList / MAL id")))
        val now = clock()
        var added = 0
        downloadsState.value = downloadsStore.update { current ->
            val fresh = chapters.map { downloadId(it) to it }.filter { (id, _) -> current.none { it.id == id && it.state != DownloadState.FAILED } }
            added = fresh.size
            current.filterNot { c -> fresh.any { it.first == c.id } } + fresh.map { (id, ch) ->
                DownloadedNovelChapter(id, ch, mediaId, media.title, media.cover, media.ref.anilistId, DownloadState.QUEUED, createdAtMs = now)
            }
        }
        startDownloads()
        return Result.success(added)
    }

    override fun downloadedChapters(mediaId: String): Flow<List<NovelChapter>> = downloadsState.map { list ->
        list.filter { it.mediaId == mediaId && it.state == DownloadState.COMPLETED }.map { it.chapter.copy(downloaded = true) }
    }

    override suspend fun removeDownloadedChapter(chapterId: String) {
        val removed = downloadsState.value.filter { it.id == chapterId || it.chapter.id == chapterId }
        downloadsState.value = downloadsStore.update { list -> list.filterNot { it in removed } }
        removed.forEach { runCatching { texts.delete(it.id) } }
    }

    override suspend fun offlineText(chapterId: String): NovelChapterText? {
        val record = downloadsState.value.firstOrNull { (it.id == chapterId || it.chapter.id == chapterId) && it.state == DownloadState.COMPLETED }
            ?: return null
        val body = runCatching { texts.get(record.id) }.getOrNull() ?: return null
        return runCatching { ReaderJson.decodeFromString(NovelChapterText.serializer(), body) }.getOrNull()
    }

    override val kind: ChapterKind = ChapterKind.NOVEL

    override fun downloads(): Flow<List<ChapterDownload>> = downloadsState.map { list -> list.map(::toChapterDownload) }

    override suspend fun remove(id: String) = removeDownloadedChapter(id)

    override suspend fun retry(id: String) {
        downloadsState.value = downloadsStore.update { list ->
            list.map { if (it.id == id && (it.state == DownloadState.FAILED || it.state == DownloadState.BROKEN)) it.copy(state = DownloadState.QUEUED, error = null) else it }
        }
        startDownloads()
    }

    private fun startDownloads() {
        if (downloadJob?.isActive == true) return
        downloadJob = scope.launch {
            downloadLock.withLock {
                while (true) {
                    val next = downloadsState.value.firstOrNull { it.state == DownloadState.QUEUED } ?: break
                    setDownload(next.id) { it.copy(state = DownloadState.DOWNLOADING, error = null) }
                    try {
                        val pages = extensions.providerRead(next.chapter.extensionId, next.chapter.sourceId, next.chapter.itemId, next.chapter.id)
                        val paragraphs = NovelText.fromPages(pages, ::fetchHtml)
                        if (paragraphs.isEmpty()) throw AppException(AppError.Extension(next.chapter.extensionId, "custom/read: no text"))
                        val body = ReaderJson.encodeToString(NovelChapterText.serializer(), NovelChapterText(next.chapter.id, next.chapter.title, paragraphs))
                        val bytes = texts.put(next.id, next.mediaId, body, clock())
                        setDownload(next.id) { it.copy(state = DownloadState.COMPLETED, bytes = bytes) }
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        setDownload(next.id) { it.copy(state = DownloadState.FAILED, error = e.message ?: e::class.java.simpleName) }
                    }
                }
            }
        }
    }

    private suspend fun setDownload(id: String, transform: (DownloadedNovelChapter) -> DownloadedNovelChapter) {
        downloadsState.value = downloadsStore.update { list -> list.map { if (it.id == id) transform(it) else it } }
    }

    private fun toChapterDownload(d: DownloadedNovelChapter): ChapterDownload {
        val args = NovelReaderArgs(
            mediaId = d.mediaId,
            title = d.title,
            poster = d.poster,
            extensionId = d.chapter.extensionId,
            sourceId = d.chapter.sourceId,
            itemId = d.chapter.itemId,
            chapterId = d.chapter.id,
            chapterNumber = d.chapter.number,
            offline = true,
            anilistId = d.anilistId,
        )
        return ChapterDownload(
            id = d.id,
            kind = ChapterKind.NOVEL,
            mediaId = d.mediaId,
            title = d.title,
            poster = d.poster,
            chapterLabel = chapterLabel(d.chapter),
            chapterNumber = d.chapter.number,
            state = d.state,
            progress = if (d.state == DownloadState.COMPLETED) 1.0 else 0.0,
            bytes = d.bytes,
            createdAtMs = d.createdAtMs,
            localRef = ReaderJson.encodeToString(NovelReaderArgs.serializer(), args),
        )
    }

    // ---------------------------------------------------------------- reset

    override suspend fun wipe() {
        downloadJob?.cancel()
        progressStore.clear()
        matchStore.clear()
        countStore.clear()
        downloadsStore.clear()
        updatesStore.clear()
        readlistStore.wipe()
        texts.clear()
        memoLock.withLock {
            infoCache.clear()
            chapterMemo.clear()
        }
        progressState.value = emptyMap()
        downloadsState.value = emptyList()
        updatesState.value = emptyList()
        feeds.value = NovelHome()
        storeDir.deleteRecursively()
    }

    // ---------------------------------------------------------------- helpers

    private suspend fun <T> load(block: suspend () -> T): LoadState<T> = try {
        LoadState.Ok(block())
    } catch (e: CancellationException) {
        throw e
    } catch (e: AppException) {
        LoadState.Err(e.error)
    } catch (e: Exception) {
        LoadState.Err(AppError.Unknown(e.message ?: e::class.java.simpleName))
    }

    private suspend fun titlesOf(ref: MediaRef, title: String): List<String> {
        val info = runCatching { info(ref) }.getOrNull()
        return (listOf(title) + listOfNotNull(info?.media?.titles?.get("romaji"), info?.media?.titles?.get("english")) + info?.synonyms.orEmpty().take(3))
            .filter { it.isNotBlank() }.distinct()
    }

    private suspend fun matchFor(key: String, find: suspend () -> String?): String? {
        val cached = matchStore.read()[key]
        if (cached != null && (cached.itemId != null || clock() - cached.atMs < MISS_TTL_MS)) return cached.itemId
        val found = try {
            find()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            return cached?.itemId
        }
        matchStore.update { it + (key to NovelMatch(found, clock())) }
        return found
    }

    private suspend fun cachedChapters(sourceKey: String, itemId: String): List<NovelChapter>? {
        val key = "$sourceKey|$itemId"
        memoLock.withLock { chapterMemo[key] }?.let { if (clock() - it.atMs < CHAPTERS_TTL_MS) return it.chapters }
        val file = chapterFile(sourceKey, itemId)
        val cached = runCatching { if (file.isFile) ReaderJson.decodeFromString(CachedNovelChapters.serializer(), file.readText()) else null }.getOrNull()
            ?: return null
        if (clock() - cached.atMs >= CHAPTERS_TTL_MS) return null
        memoLock.withLock { chapterMemo[key] = cached }
        return cached.chapters
    }

    private fun markDownloaded(list: List<NovelChapter>): List<NovelChapter> {
        val done = downloadsState.value.filter { it.state == DownloadState.COMPLETED }.map { it.id }.toSet()
        if (done.isEmpty()) return list
        return list.map { if (downloadId(it) in done) it.copy(downloaded = true) else it }
    }

    private fun chapterFile(sourceKey: String, itemId: String): File = File(storeDir, "chapter-lists/${safeFileName("$sourceKey|$itemId")}.json")

    companion object {
        const val DIR_NAME: String = "novels"

        /** AniList `MediaFormat` of light novels (`MediaDetails.format`). */
        const val NOVEL_FORMAT: String = "NOVEL"
        private const val HERO_SIZE = 8
        private const val MAX_FOLLOWED = 12
        private const val SOURCE_TIMEOUT_MS = 15_000L
        private const val CHAPTERS_TTL_MS = 6 * 60 * 60 * 1000L
        private const val MISS_TTL_MS = 24 * 60 * 60 * 1000L
        private const val UPDATE_TTL_MS = 7 * 24 * 60 * 60 * 1000L

        /** A chapter counts as read when the reader passed this fraction of it. */
        const val COMPLETED_OFFSET: Float = 0.95f

        fun downloadId(chapter: NovelChapter): String = "${chapter.extensionId}/${chapter.sourceId}/${chapter.id}"

        fun mediaKey(ref: MediaRef): String? = HistoryEntry.animeIdOf(ref.anilistId, ref.malId)

        fun refOf(mediaId: String): MediaRef? = when {
            mediaId.startsWith("mal:") -> mediaId.removePrefix("mal:").toIntOrNull()?.let { MediaRef(malId = it) }
            else -> mediaId.toIntOrNull()?.let { MediaRef(anilistId = it) }
        }

        fun chapterLabel(chapter: NovelChapter): String = buildString {
            ChapterText.formatNumber(chapter.number)?.let { append("Ch. ").append(it) }
            if (isEmpty()) append(chapter.title ?: chapter.id) else chapter.title?.takeIf { it.isNotBlank() }?.let { append(" · ").append(it) }
        }
    }
}
