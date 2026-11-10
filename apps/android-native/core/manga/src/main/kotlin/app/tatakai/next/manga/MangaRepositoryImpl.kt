// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

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
import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import app.tatakai.next.model.MangaDetails
import app.tatakai.next.model.MangaHome
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.MangaRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.model.ReadListPuller
import app.tatakai.next.model.ReadingProgress
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
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.builtins.ListSerializer
import okhttp3.OkHttpClient
import java.io.File
import java.util.Locale

/** Where a title was found inside one source (`custom/search` match), cached in `storeDir/matches.json`. */
@Serializable
internal data class SourceMatch(val itemId: String? = null, val atMs: Long = 0)

@Serializable
internal data class CachedChapters(val chapters: List<MangaChapter> = emptyList(), val atMs: Long = 0)

@Serializable
internal data class CachedSeries(val series: SeriesDto? = null, val atMs: Long = 0)

@Serializable
internal data class SeriesDto(
    val id: Long,
    val title: String,
    val status: String? = null,
    val completed: Boolean = false,
    val licensed: Boolean = false,
    val publishers: List<String> = emptyList(),
    val serialization: String? = null,
    val rating: Int? = null,
    val year: Int? = null,
    val latestChapter: Int? = null,
    val url: String? = null,
) {
    fun toModel() = MangaUpdatesSeries(id, title, status, completed, licensed, publishers, serialization, rating, year, latestChapter, url)

    companion object {
        fun of(s: MangaUpdatesSeries) = SeriesDto(s.id, s.title, s.status, s.completed, s.licensed, s.publishers, s.serialization, s.rating, s.year, s.latestChapter, s.url)
    }
}

/** A chapter queued / downloaded to `storeDir/chapters/<id>/`. */
@Serializable
internal data class DownloadedChapter(
    val id: String,
    val chapter: MangaChapter,
    val mediaId: String,
    val title: String,
    val poster: String? = null,
    val anilistId: Int? = null,
    val state: DownloadState = DownloadState.QUEUED,
    val progress: Double = 0.0,
    val bytes: Long = 0,
    val pages: Int = 0,
    val createdAtMs: Long,
    val error: String? = null,
)

/** A followed title (READING) with a chapter newer than the last one seen (classic FollowedMangaUpdates). */
@Serializable
internal data class FollowedUpdate(val entry: ReadEntry, val chapter: MangaChapter, val foundAtMs: Long)

@Serializable
internal data class UpdatesDoc(
    /** mediaId -> highest chapter number seen at the last check. */
    val seen: Map<String, Double> = emptyMap(),
    val updates: List<FollowedUpdate> = emptyList(),
)

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.5, worker s): [MangaRepository] + the manga [ChapterDownloadSource]
 * + [MangaCatalog] (extras of the screens).
 *
 * - Metadata: AniList through the private [AniListMangaApi] (home rows, search with origin / status / genre / year / sort, details with
 *   chapters, volumes, characters, relations), MangaUpdates ([MangaUpdatesClient]: publishers, serialization, status) and MangaDex
 *   ([MangaDexClient]: chapter list when no manga extension is installed, as the synthetic source `mangadex`).
 * - Chapters: `ExtensionRepository.customSources("read")` + `providerSearch` (title match, cached in `matches.json`) + `providerInfo`
 *   (entries of kind `read`, cached 6 h in `chapters/`) + `providerRead` (pages). Sources are ranked by preferred source, preferred
 *   languages and reliability ([SourcePicker]); the reliability moving average lives in `reliability.json`.
 * - Progress (`progress.json`), readlist ([ReadlistStore], AniList `SaveMediaListEntry` when linked), followed updates (`updates.json`),
 *   downloaded pages (`chapters/<id>/0001.jpg`, index in `downloads.json`), private prefs (`prefs.json`).
 */
class MangaRepositoryImpl(
    private val metadata: MetadataRepository,
    private val extensions: ExtensionRepository,
    private val lists: ListsRepository? = null,
    private val client: OkHttpClient,
    private val storeDir: File,
    private val settings: () -> AppSettings = { AppSettings() },
    private val clock: () -> Long = System::currentTimeMillis,
) : MangaRepository, ChapterDownloadSource, AppDataWiper, MangaCatalog, ReadListPuller {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val anilist = AniListMangaApi(client)
    private val mangaUpdatesClient = MangaUpdatesClient(client)
    private val mangaDex = MangaDexClient(client)

    private val progressStore = JsonFileStore(File(storeDir, "progress.json"), MapSerializer(String.serializer(), ReadingProgress.serializer())) { emptyMap() }
    private val prefsStore = JsonFileStore(File(storeDir, "prefs.json"), MangaPrefs.serializer()) { MangaPrefs() }
    private val reliabilityStore = JsonFileStore(File(storeDir, "reliability.json"), MapSerializer(String.serializer(), Float.serializer())) { emptyMap() }
    private val matchStore = JsonFileStore(File(storeDir, "matches.json"), MapSerializer(String.serializer(), SourceMatch.serializer())) { emptyMap() }
    private val seriesStore = JsonFileStore(File(storeDir, "mangaupdates.json"), MapSerializer(String.serializer(), CachedSeries.serializer())) { emptyMap() }
    private val downloadsStore = JsonFileStore(File(storeDir, "downloads.json"), ListSerializer(DownloadedChapter.serializer())) { emptyList() }
    private val updatesStore = JsonFileStore(File(storeDir, "updates.json"), UpdatesDoc.serializer()) { UpdatesDoc() }
    private val readlistStore = ReadlistStore(
        File(storeDir, "readlist.json"), anilist, lists, novels = false, clock = clock, writeBack = { settings().listsWriteBack },
    )

    private val progressState = MutableStateFlow<Map<String, ReadingProgress>>(emptyMap())
    private val downloadsState = MutableStateFlow<List<DownloadedChapter>>(emptyList())
    private val updatesState = MutableStateFlow<List<FollowedUpdate>>(emptyList())
    private val _prefs = MutableStateFlow(MangaPrefs())
    override val prefs: StateFlow<MangaPrefs> = _prefs.asStateFlow()
    private val reliability = MutableStateFlow<Map<String, Float>>(emptyMap())

    private val feeds = MutableStateFlow(MangaHome())
    private val infoCache = HashMap<String, MangaInfo>()
    private val chapterMemo = HashMap<String, CachedChapters>()
    private val memoLock = Mutex()
    private var genresCache: List<String>? = null
    private var downloadJob: Job? = null
    private val downloadLock = Mutex()

    init {
        scope.launch {
            progressState.value = progressStore.read()
            downloadsState.value = downloadsStore.read().map { if (it.state == DownloadState.DOWNLOADING) it.copy(state = DownloadState.QUEUED) else it }
            updatesState.value = updatesStore.read().updates
            _prefs.value = prefsStore.read()
            reliability.value = reliabilityStore.read()
            readlistStore.load()
            if (downloadsState.value.any { it.state == DownloadState.QUEUED }) startDownloads()
        }
    }

    // ---------------------------------------------------------------- home / search / details

    override fun home(): Flow<MangaHome> = combine(feeds, lastRead(20), updatesState) { home, last, updates ->
        home.copy(lastRead = last, followedUpdates = updates.map { it.entry to it.chapter })
    }

    override suspend fun refreshHome() {
        feeds.update { it.copy(hero = LoadState.Loading, trending = LoadState.Loading, popular = LoadState.Loading, topRated = LoadState.Loading) }
        coroutineScope {
            val trending = async { load { anilist.search(MediaSearchQuery(sort = MediaSort.TRENDING), 1, novels = false).items } }
            val popular = async { load { anilist.search(MediaSearchQuery(sort = MediaSort.POPULARITY), 1, novels = false).items } }
            val top = async { load { anilist.search(MediaSearchQuery(sort = MediaSort.SCORE), 1, novels = false).items } }
            val genres = async { runCatching { genres() }.getOrDefault(emptyList()) }
            val t = trending.await()
            val hero = when (t) {
                is LoadState.Ok -> LoadState.Ok(t.value.sortedByDescending { it.banner != null }.take(HERO_SIZE))
                else -> t
            }
            feeds.update { it.copy(hero = hero, trending = t, popular = popular.await(), topRated = top.await(), genres = genres.await()) }
        }
        scope.launch { runCatching { checkFollowedUpdates() } }
        if (feeds.value.adult != null) runCatching { loadAdultZone() }
    }

    override suspend fun loadAdultZone() {
        if (!settings().showAdult || !_prefs.value.adultGateAccepted) {
            feeds.update { it.copy(adult = null) }
            return
        }
        feeds.update { it.copy(adult = LoadState.Loading) }
        val state = load { anilist.search(MediaSearchQuery(sort = MediaSort.POPULARITY, adult = true), 1, novels = false).items }
        feeds.update { it.copy(adult = state) }
    }

    override suspend fun search(query: MediaSearchQuery, page: Int): FeedPage =
        anilist.search(query.copy(adult = query.adult && settings().showAdult), page, novels = false)

    override suspend fun info(ref: MediaRef): MangaInfo? {
        val key = mediaKey(ref) ?: return null
        memoLock.withLock { infoCache[key] }?.let { return it }
        val info = anilist.info(ref) ?: return null
        memoLock.withLock { infoCache[key] = info }
        return info
    }

    override suspend fun mangaUpdates(ref: MediaRef): MangaUpdatesSeries? {
        val key = mediaKey(ref) ?: return null
        val cached = seriesStore.read()[key]
        if (cached != null && clock() - cached.atMs < SERIES_TTL_MS) return cached.series?.toModel()
        val info = runCatching { info(ref) }.getOrNull() ?: return cached?.series?.toModel()
        val titles = listOfNotNull(info.media.titles["romaji"], info.media.titles["english"], info.media.title) + info.synonyms
        val series = try {
            mangaUpdatesClient.find(titles, info.media.year)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            return cached?.series?.toModel()
        }
        seriesStore.update { it + (key to CachedSeries(series?.let(SeriesDto::of), clock())) }
        return series
    }

    override suspend fun details(ref: MediaRef): MangaDetails? {
        val info = info(ref) ?: return null
        val mu = runCatching { mangaUpdates(ref) }.getOrNull()
        val ratings = buildMap {
            info.media.score?.let { put("anilist", it) }
            mu?.rating?.let { put("mangaupdates", it) }
        }
        val adaptation = info.relations.firstOrNull { it.relation == "ADAPTATION" && it.media.kind == MediaKind.ANIME }?.media?.ref
            ?: info.relations.firstOrNull { it.media.kind == MediaKind.ANIME }?.media?.ref
        return MangaDetails(
            ref = info.media.ref,
            chapters = info.chapters ?: mu?.latestChapter,
            volumes = info.volumes,
            publicationStatus = info.status,
            origin = info.origin,
            publishers = mu?.publishers.orEmpty(),
            serialization = mu?.serialization,
            mangaUpdatesId = mu?.id,
            ratings = ratings,
            animeAdaptation = adaptation,
        )
    }

    override suspend fun genres(): List<String> {
        genresCache?.let { return it }
        val list = anilist.genres().filterNot { it.equals("Hentai", ignoreCase = true) && !settings().showAdult }
        genresCache = list
        return list
    }

    // ---------------------------------------------------------------- sources / chapters

    override fun preferredLanguages(): List<String> {
        val app = settings().language.tag ?: Locale.getDefault().language
        return SourcePicker.languages(app, _prefs.value.languages)
    }

    override suspend fun chapterSources(ref: MediaRef, title: String): List<MangaChapterSource> {
        val key = mediaKey(ref) ?: return emptyList()
        val titles = titlesOf(ref, title)
        val custom = runCatching { extensions.customSources("read") }.getOrDefault(emptyList())
        val rel = reliability.value
        val sources = if (custom.isEmpty()) {
            val id = matchFor(key, sourceKey(MangaDexClient.EXTENSION_ID, MangaDexClient.SOURCE_ID)) {
                mangaDex.find(ref.anilistId, titles)
            }
            listOf(
                MangaChapterSource(
                    extensionId = MangaDexClient.EXTENSION_ID,
                    extensionName = MangaDexClient.NAME,
                    sourceId = MangaDexClient.SOURCE_ID,
                    itemId = id,
                    languages = preferredLanguages(),
                    chapterCount = id?.let { cachedChapters(sourceKey(MangaDexClient.EXTENSION_ID, MangaDexClient.SOURCE_ID), it)?.size },
                    reliability = rel[sourceKey(MangaDexClient.EXTENSION_ID, MangaDexClient.SOURCE_ID)] ?: app.tatakai.next.model.INITIAL_RELIABILITY,
                ),
            )
        } else {
            coroutineScope {
                custom.map { src ->
                    async {
                        val sk = sourceKey(src.extensionId, src.sourceId)
                        val itemId = withTimeoutOrNull(SOURCE_TIMEOUT_MS) {
                            matchFor(key, sk) {
                                val hits = titles.take(2).flatMap { t -> runCatching { extensions.providerSearch(src.extensionId, src.sourceId, t) }.getOrDefault(emptyList()) }
                                TitleMatch.best(hits, titles)?.id
                            }
                        }
                        val cached = itemId?.let { cachedChapters(sk, it) }
                        MangaChapterSource(
                            extensionId = src.extensionId,
                            extensionName = src.extensionName,
                            sourceId = src.sourceId,
                            itemId = itemId,
                            languages = cached?.let(ChapterLists::languages).orEmpty(),
                            chapterCount = cached?.size,
                            reliability = rel[sk] ?: app.tatakai.next.model.INITIAL_RELIABILITY,
                        )
                    }
                }.awaitAll()
            }
        }
        val prefs = _prefs.value
        return SourcePicker.rank(sources, preferredLanguages(), prefs.preferredSourceByMedia[key] ?: prefs.preferredSource)
    }

    override suspend fun chapters(source: MangaChapterSource, refresh: Boolean): List<MangaChapter> {
        val itemId = source.itemId ?: return emptyList()
        return chaptersOf(source.extensionId, source.sourceId, itemId, refresh)
    }

    override suspend fun chaptersOf(extensionId: String, sourceId: String, itemId: String): List<MangaChapter> =
        chaptersOf(extensionId, sourceId, itemId, refresh = false)

    private suspend fun chaptersOf(extensionId: String, sourceId: String, itemId: String, refresh: Boolean): List<MangaChapter> {
        val sk = sourceKey(extensionId, sourceId)
        if (!refresh) cachedChapters(sk, itemId)?.let { return markDownloaded(it) }
        val list = if (extensionId == MangaDexClient.EXTENSION_ID) {
            mangaDex.chapters(itemId, preferredLanguages())
        } else {
            val info = extensions.providerInfo(extensionId, sourceId, itemId)
                ?: throw AppException(AppError.Extension(extensionId, "custom/info: nothing"))
            info.entries
                .filter { it.kind.isBlank() || it.kind == "read" }
                .map { e ->
                    MangaChapter(
                        id = e.id,
                        extensionId = extensionId,
                        sourceId = sourceId,
                        itemId = itemId,
                        number = e.number ?: ChapterText.number(e.title),
                        volume = ChapterText.volume(e.title),
                        title = e.title,
                        lang = e.lang?.lowercase(Locale.ROOT),
                        group = ChapterText.group(e.title),
                        releasedAtMs = e.releasedAtMs,
                    )
                }
        }
        val cached = CachedChapters(list, clock())
        memoLock.withLock { chapterMemo["$sk|$itemId"] = cached }
        runCatching {
            val file = chapterFile(sk, itemId)
            file.parentFile?.mkdirs()
            file.writeText(ReaderJson.encodeToString(CachedChapters.serializer(), cached))
        }
        return markDownloaded(list)
    }

    override suspend fun sourceOf(extensionId: String, sourceId: String, itemId: String): MangaChapterSource {
        val sk = sourceKey(extensionId, sourceId)
        val name = if (extensionId == MangaDexClient.EXTENSION_ID) {
            MangaDexClient.NAME
        } else {
            runCatching { extensions.customSources("read") }.getOrDefault(emptyList())
                .firstOrNull { it.extensionId == extensionId && it.sourceId == sourceId }?.let { it.extensionName.ifBlank { it.name } }
                ?: extensions.installed.value.firstOrNull { it.id == extensionId }?.name
                ?: extensionId
        }
        val cached = cachedChapters(sk, itemId)
        return MangaChapterSource(
            extensionId = extensionId,
            extensionName = name,
            sourceId = sourceId,
            itemId = itemId,
            languages = cached?.let(ChapterLists::languages).orEmpty(),
            chapterCount = cached?.size,
            reliability = reliability.value[sk] ?: app.tatakai.next.model.INITIAL_RELIABILITY,
        )
    }

    override suspend fun bestChapter(ref: MediaRef, title: String, number: Double): MangaChapter? {
        val sources = chapterSources(ref, title).filter { it.itemId != null }.take(MAX_BEST_SOURCES)
        val candidates = coroutineScope {
            sources.map { s -> async { s to (withTimeoutOrNull(SOURCE_TIMEOUT_MS) { runCatching { chapters(s) }.getOrNull() } ?: emptyList()) } }.awaitAll()
        }
        val key = mediaKey(ref)
        val prefs = _prefs.value
        return SourcePicker.bestChapter(candidates, number, preferredLanguages(), key?.let { prefs.preferredSourceByMedia[it] } ?: prefs.preferredSource)
    }

    override suspend fun pages(chapter: MangaChapter): List<ChapterPage> {
        downloadsState.value.firstOrNull { it.chapter.id == chapter.id && it.chapter.sourceKey == chapter.sourceKey && it.state == DownloadState.COMPLETED }
            ?.let { done -> PageFiles.list(chapterDir(done.id)).takeIf { it.isNotEmpty() }?.let { return it } }
        val sk = chapter.sourceKey
        return try {
            val pages = if (chapter.extensionId == MangaDexClient.EXTENSION_ID) {
                mangaDex.pages(chapter.id)
            } else {
                extensions.providerRead(chapter.extensionId, chapter.sourceId, chapter.itemId, chapter.id)
            }
            reportPageResult(sk, pages.isNotEmpty())
            if (pages.isEmpty()) throw AppException(AppError.Extension(chapter.extensionId, "custom/read: no pages"))
            pages.sortedBy { it.index }
        } catch (e: CancellationException) {
            throw e
        } catch (e: AppException) {
            reportPageResult(sk, false)
            throw e
        } catch (e: Exception) {
            reportPageResult(sk, false)
            throw AppException(AppError.Extension(chapter.extensionId, e.message ?: e::class.java.simpleName), e)
        }
    }

    override suspend fun reportPageResult(sourceKey: String, ok: Boolean) {
        val next = reliabilityStore.update { map -> map + (sourceKey to SourcePicker.updateReliability(map[sourceKey], ok)) }
        reliability.value = next
    }

    override suspend fun clearChapterCache(ref: MediaRef) {
        val key = mediaKey(ref) ?: return
        val matches = matchStore.read().filterKeys { it.startsWith("$key|") }
        matches.forEach { (k, m) ->
            val sk = k.substringAfter('|')
            val itemId = m.itemId ?: return@forEach
            memoLock.withLock { chapterMemo.remove("$sk|$itemId") }
            runCatching { chapterFile(sk, itemId).delete() }
        }
        matchStore.update { map -> map.filterKeys { !it.startsWith("$key|") } }
        memoLock.withLock { infoCache.remove(key) }
    }

    // ---------------------------------------------------------------- progress / readlist

    override fun lastRead(limit: Int): Flow<List<ReadingProgress>> =
        progressState.map { map -> map.values.sortedByDescending { it.updatedAtMs }.take(limit) }

    override fun progress(mediaId: String): Flow<ReadingProgress?> = progressState.map { it[mediaId] }

    override suspend fun saveProgress(progress: ReadingProgress) {
        progressState.value = progressStore.update { it + (progress.mediaId to progress) }
        val number = progress.chapterNumber
        if (number != null) {
            val updates = updatesStore.update { doc ->
                val seen = maxOf(doc.seen[progress.mediaId] ?: 0.0, number)
                doc.copy(seen = doc.seen + (progress.mediaId to seen), updates = doc.updates.filterNot { mediaKey(it.entry.ref) == progress.mediaId && (it.chapter.number ?: 0.0) <= seen })
            }
            updatesState.value = updates.updates
        }
        if (progress.completed && number != null) {
            val ref = refOf(progress.mediaId) ?: return
            val chapters = number.toInt()
            val current = readlistStore.current(ref)
            if (current == null || current.progress < chapters) {
                readlistStore.setProgress(ref, chapters, progress.title, progress.poster)
            }
        }
    }

    override fun readlist(status: ListStatus?): Flow<List<ReadEntry>> = readlistStore.flow(status)

    override fun readEntry(ref: MediaRef): Flow<ReadEntry?> = readlistStore.entry(ref)

    override suspend fun setReadStatus(media: Media, status: ListStatus?): Result<Unit> = readlistStore.setStatus(media, status)

    override suspend fun setReadProgress(ref: MediaRef, chapters: Int): Result<Unit> = readlistStore.setProgress(ref, chapters)

    override suspend fun pullReadlist(): Result<Int> = readlistStore.pull()

    /** "Sincronizar ahora" of the lists (ListsRepository.syncNow) refreshes the manga read list too. */
    override suspend fun pullReadList(): Result<Int> = readlistStore.pull()

    override suspend fun updatePrefs(transform: (MangaPrefs) -> MangaPrefs) {
        _prefs.value = prefsStore.update(transform)
    }

    override suspend fun dismissUpdate(mediaId: String) {
        updatesState.value = updatesStore.update { doc -> doc.copy(updates = doc.updates.filterNot { mediaKey(it.entry.ref) == mediaId }) }.updates
    }

    /** Followed (READING) titles: newest chapter of the best source vs the last number seen. */
    private suspend fun checkFollowedUpdates() {
        val reading = readlistStore.entries.value.filter { it.status == ListStatus.WATCHING || it.status == ListStatus.REPEATING }.take(MAX_FOLLOWED)
        if (reading.isEmpty()) return
        val now = clock()
        val found = mutableListOf<FollowedUpdate>()
        val latest = mutableMapOf<String, Double>()
        reading.forEach { entry ->
            val key = mediaKey(entry.ref) ?: return@forEach
            val source = runCatching { chapterSources(entry.ref, entry.title).firstOrNull { it.itemId != null } }.getOrNull() ?: return@forEach
            val chapters = runCatching { chapters(source, refresh = true) }.getOrNull() ?: return@forEach
            val top = ChapterLists.sorted(chapters, ChapterOrder.DESC).firstOrNull { it.number != null } ?: return@forEach
            latest[key] = top.number ?: return@forEach
            found += FollowedUpdate(entry, top, now)
        }
        val doc = updatesStore.update { doc ->
            val fresh = found.filter { u ->
                val key = mediaKey(u.entry.ref) ?: return@filter false
                val seen = doc.seen[key] ?: maxOf(progressState.value[key]?.chapterNumber ?: 0.0, u.entry.progress.toDouble())
                (u.chapter.number ?: 0.0) > seen
            }
            val kept = doc.updates.filter { now - it.foundAtMs < UPDATE_TTL_MS && fresh.none { f -> mediaKey(f.entry.ref) == mediaKey(it.entry.ref) } }
            doc.copy(seen = doc.seen + latest.filterKeys { it !in doc.seen }, updates = (fresh + kept).sortedByDescending { it.foundAtMs })
        }
        updatesState.value = doc.updates
    }

    // ---------------------------------------------------------------- downloads

    override suspend fun downloadChapters(media: Media, chapters: List<MangaChapter>): Result<Int> {
        val mediaId = HistoryEntry.animeIdOf(media.ref.anilistId, media.ref.malId)
            ?: return Result.failure(AppException(AppError.Unknown("no AniList / MAL id")))
        val now = clock()
        var added = 0
        val list = downloadsStore.update { current ->
            val fresh = chapters.map { ch -> downloadId(ch) to ch }.filter { (id, _) -> current.none { it.id == id && it.state != DownloadState.FAILED } }
            added = fresh.size
            current.filterNot { c -> fresh.any { it.first == c.id } } + fresh.map { (id, ch) ->
                DownloadedChapter(id, ch, mediaId, media.title, media.cover, media.ref.anilistId, DownloadState.QUEUED, createdAtMs = now)
            }
        }
        downloadsState.value = list
        startDownloads()
        return Result.success(added)
    }

    override fun downloadedChapters(mediaId: String): Flow<List<MangaChapter>> = downloadsState.map { list ->
        list.filter { it.mediaId == mediaId && it.state == DownloadState.COMPLETED }.map { it.chapter.copy(downloaded = true) }
    }

    override suspend fun removeDownloadedChapter(chapterId: String) {
        val removed = downloadsState.value.filter { it.id == chapterId || it.chapter.id == chapterId }
        downloadsState.value = downloadsStore.update { list -> list.filterNot { it in removed } }
        removed.forEach { runCatching { chapterDir(it.id).deleteRecursively() } }
    }

    override suspend fun offlinePages(chapterId: String): List<ChapterPage> {
        val record = downloadsState.value.firstOrNull { (it.id == chapterId || it.chapter.id == chapterId) && it.state == DownloadState.COMPLETED }
            ?: return emptyList()
        return PageFiles.list(chapterDir(record.id))
    }

    override val kind: ChapterKind = ChapterKind.MANGA

    override fun downloads(): Flow<List<ChapterDownload>> = downloadsState.map { list -> list.map(::toChapterDownload) }

    override suspend fun remove(id: String) = removeDownloadedChapter(id)

    override suspend fun retry(id: String) {
        downloadsState.value = downloadsStore.update { list ->
            list.map { if (it.id == id && (it.state == DownloadState.FAILED || it.state == DownloadState.BROKEN)) it.copy(state = DownloadState.QUEUED, error = null, progress = 0.0) else it }
        }
        startDownloads()
    }

    private fun startDownloads() {
        if (downloadJob?.isActive == true) return
        downloadJob = scope.launch {
            downloadLock.withLock {
                while (true) {
                    val next = downloadsState.value.firstOrNull { it.state == DownloadState.QUEUED } ?: break
                    runDownload(next)
                }
            }
        }
    }

    private suspend fun runDownload(item: DownloadedChapter) {
        setDownload(item.id) { it.copy(state = DownloadState.DOWNLOADING, progress = 0.0, error = null) }
        val dir = chapterDir(item.id)
        try {
            val pages = when (item.chapter.extensionId) {
                MangaDexClient.EXTENSION_ID -> mangaDex.pages(item.chapter.id)
                else -> extensions.providerRead(item.chapter.extensionId, item.chapter.sourceId, item.chapter.itemId, item.chapter.id)
            }
            if (pages.isEmpty()) throw AppException(AppError.Extension(item.chapter.extensionId, "custom/read: no pages"))
            var bytes = 0L
            pages.sortedBy { it.index }.forEachIndexed { i, page ->
                if (downloadsState.value.none { it.id == item.id }) return // removed meanwhile
                bytes += PageFiles.download(client, page.copy(index = i), dir)
                val done = (i + 1).toDouble() / pages.size
                setDownload(item.id) { it.copy(progress = done, bytes = bytes, pages = pages.size) }
            }
            setDownload(item.id) { it.copy(state = DownloadState.COMPLETED, progress = 1.0, bytes = bytes, pages = pages.size) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            setDownload(item.id) { it.copy(state = DownloadState.FAILED, error = e.message ?: e::class.java.simpleName) }
        }
    }

    private suspend fun setDownload(id: String, transform: (DownloadedChapter) -> DownloadedChapter) {
        downloadsState.value = downloadsStore.update { list -> list.map { if (it.id == id) transform(it) else it } }
    }

    private fun toChapterDownload(d: DownloadedChapter): ChapterDownload {
        val args = MangaReaderArgs(
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
            kind = ChapterKind.MANGA,
            mediaId = d.mediaId,
            title = d.title,
            poster = d.poster,
            chapterLabel = chapterLabel(d.chapter),
            chapterNumber = d.chapter.number,
            state = d.state,
            progress = d.progress,
            bytes = d.bytes,
            createdAtMs = d.createdAtMs,
            localRef = ReaderJson.encodeToString(MangaReaderArgs.serializer(), args),
        )
    }

    // ---------------------------------------------------------------- reset

    /** "Restablecer la app": progress, readlist, caches, prefs and downloaded pages. */
    override suspend fun wipe() {
        downloadJob?.cancel()
        progressStore.clear()
        prefsStore.clear()
        reliabilityStore.clear()
        matchStore.clear()
        seriesStore.clear()
        downloadsStore.clear()
        updatesStore.clear()
        readlistStore.wipe()
        memoLock.withLock {
            infoCache.clear()
            chapterMemo.clear()
        }
        progressState.value = emptyMap()
        downloadsState.value = emptyList()
        updatesState.value = emptyList()
        _prefs.value = MangaPrefs()
        reliability.value = emptyMap()
        feeds.value = MangaHome()
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

    /** Cached source match of a title; a miss is retried after a day. */
    private suspend fun matchFor(mediaKey: String, sourceKey: String, find: suspend () -> String?): String? {
        val key = "$mediaKey|$sourceKey"
        val cached = matchStore.read()[key]
        if (cached != null && (cached.itemId != null || clock() - cached.atMs < MISS_TTL_MS)) return cached.itemId
        val found = try {
            find()
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            return cached?.itemId
        }
        matchStore.update { it + (key to SourceMatch(found, clock())) }
        return found
    }

    private suspend fun cachedChapters(sourceKey: String, itemId: String): List<MangaChapter>? {
        val key = "$sourceKey|$itemId"
        memoLock.withLock { chapterMemo[key] }?.let { if (clock() - it.atMs < CHAPTERS_TTL_MS) return it.chapters }
        val file = chapterFile(sourceKey, itemId)
        val cached = runCatching { if (file.isFile) ReaderJson.decodeFromString(CachedChapters.serializer(), file.readText()) else null }.getOrNull()
            ?: return null
        if (clock() - cached.atMs >= CHAPTERS_TTL_MS) return null
        memoLock.withLock { chapterMemo[key] = cached }
        return cached.chapters
    }

    private fun markDownloaded(list: List<MangaChapter>): List<MangaChapter> {
        val done = downloadsState.value.filter { it.state == DownloadState.COMPLETED }.map { it.id }.toSet()
        if (done.isEmpty()) return list
        return list.map { if (downloadId(it) in done) it.copy(downloaded = true) else it }
    }

    private fun chapterFile(sourceKey: String, itemId: String): File = File(storeDir, "chapter-lists/${safeFileName("$sourceKey|$itemId")}.json")

    private fun chapterDir(downloadId: String): File = File(storeDir, "chapters/${safeFileName(downloadId)}")

    companion object {
        const val DIR_NAME: String = "manga"
        const val MANGAUPDATES_BASE_URL: String = MangaUpdatesClient.BASE_URL
        const val MANGADEX_BASE_URL: String = MangaDexClient.BASE_URL
        private const val HERO_SIZE = 8
        private const val MAX_FOLLOWED = 12
        private const val MAX_BEST_SOURCES = 5
        private const val SOURCE_TIMEOUT_MS = 15_000L
        private const val CHAPTERS_TTL_MS = 6 * 60 * 60 * 1000L
        private const val SERIES_TTL_MS = 7 * 24 * 60 * 60 * 1000L
        private const val MISS_TTL_MS = 24 * 60 * 60 * 1000L
        private const val UPDATE_TTL_MS = 7 * 24 * 60 * 60 * 1000L

        /** `<ext>/<source>/<chapterId>`: unique across sources (the id of [ChapterDownload]). */
        fun downloadId(chapter: MangaChapter): String = "${chapter.extensionId}/${chapter.sourceId}/${chapter.id}"

        /** `HistoryEntry.animeIdOf` style key of a title (`"123"` / `"mal:5"`). */
        fun mediaKey(ref: MediaRef): String? = HistoryEntry.animeIdOf(ref.anilistId, ref.malId)

        fun refOf(mediaId: String): MediaRef? = when {
            mediaId.startsWith("mal:") -> mediaId.removePrefix("mal:").toIntOrNull()?.let { MediaRef(malId = it) }
            else -> mediaId.toIntOrNull()?.let { MediaRef(anilistId = it) }
        }

        fun chapterLabel(chapter: MangaChapter): String = buildString {
            chapter.volume?.let { append("Vol. ").append(it).append(' ') }
            ChapterText.formatNumber(chapter.number)?.let { append("Ch. ").append(it) }
            if (isEmpty()) append(chapter.title ?: chapter.id) else chapter.title?.takeIf { it.isNotBlank() }?.let { append(" · ").append(it) }
        }
    }
}
