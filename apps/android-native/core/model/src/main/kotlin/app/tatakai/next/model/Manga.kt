// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.5): manga (metadata, home, search, details, chapters
// by extension, reader, readlist, chapter downloads). Desktop counterpart: tk-core/manga/** (worker l). Signatures FINAL; bodies: worker s
// (:core:manga `MangaRepositoryImpl`, bound in `mangaModule`; UI in :feature:manga).
//
// Titles are `Media` with `kind = MediaKind.MANGA` (AniList; `Media.episodes` = chapters when known). Novels are AniList MANGA with
// `format == "NOVEL"` (`MediaDetails.format`): there is NO `MediaKind.NOVEL` (adding an enum value would break every exhaustive `when` of
// wave A), `NovelRepository` filters by format. Readlist statuses reuse `ListStatus` (AniList uses the same `MediaListStatus` for manga;
// labels `read_status_*` in :core:ui, `ListStatus.readLabelRes()`).

/** A manga extension source (a `.kai` custom source of kind `read`, through `ExtensionRepository.customSources("read")`). */
data class MangaChapterSource(
    val extensionId: String,
    val extensionName: String,
    val sourceId: String,
    /** The extension's own id of this title (from its `custom/search` match), null until matched. */
    val itemId: String?,
    val languages: List<String>,
    val chapterCount: Int?,
    /** 0..1 moving average of successful page loads (private store of :core:manga). */
    val reliability: Float = INITIAL_RELIABILITY,
)

/** One chapter as listed by one extension (`custom/info` entries of kind `read`). [id] is unique within (extensionId, sourceId). */
@Serializable
data class MangaChapter(
    val id: String,
    val extensionId: String,
    val sourceId: String,
    val itemId: String,
    val number: Double? = null,
    val volume: Int? = null,
    val title: String? = null,
    /** ISO 639-1 when the extension says; null = unknown. */
    val lang: String? = null,
    /** Scanlation group. */
    val group: String? = null,
    val releasedAtMs: Long? = null,
    /** Pages already downloaded (`MangaRepository.downloadedChapters`). */
    val downloaded: Boolean = false,
)

/** One page image of a chapter (`custom/read`). Headers travel with the URL (Referer etc.). */
@Serializable
data class ChapterPage(val index: Int, val url: String, val headers: Map<String, String> = emptyMap())

/** Extras of a manga title beyond `Media` / `MediaDetails` (MangaUpdates / AniList). */
data class MangaDetails(
    val ref: MediaRef,
    val chapters: Int? = null,
    val volumes: Int? = null,
    /** AniList `MediaStatus` of the publication (`RELEASING`, `FINISHED`...). */
    val publicationStatus: String? = null,
    /** `JP` / `KR` / `CN` (AniList `countryOfOrigin`). */
    val origin: String? = null,
    val publishers: List<String> = emptyList(),
    val serialization: String? = null,
    /** MangaUpdates id when matched. */
    val mangaUpdatesId: Long? = null,
    /** Cross-site scores 0..100 keyed by provider id (`anilist`, `mal`, `mangaupdates`). */
    val ratings: Map<String, Int> = emptyMap(),
    /** The anime adaptation (`relations` ADAPTATION), when any. */
    val animeAdaptation: MediaRef? = null,
)

/** Where the user left a chapter (classic "Continue reading"). [mediaId] = `HistoryEntry.animeIdOf(anilistId, malId)`. */
@Serializable
data class ReadingProgress(
    val mediaId: String,
    val title: String,
    val poster: String? = null,
    val chapterId: String,
    val chapterNumber: Double? = null,
    val extensionId: String,
    val sourceId: String,
    val itemId: String,
    val page: Int,
    val totalPages: Int,
    val updatedAtMs: Long,
) {
    val completed: Boolean get() = totalPages > 0 && page >= totalPages - 1
}

/** One row of the manga readlist (local first; synced to AniList progress when linked, through `ListsRepository.accessToken`). */
@Serializable
data class ReadEntry(
    val ref: MediaRef,
    val title: String,
    val poster: String? = null,
    val status: ListStatus,
    /** Chapters read. */
    val progress: Int = 0,
    val totalChapters: Int? = null,
    val score: Int? = null,
    val updatedAtMs: Long,
)

@Serializable
enum class ReaderMode {
    @SerialName("paged") PAGED,
    @SerialName("vertical") VERTICAL,
    @SerialName("webtoon") WEBTOON,
}

@Serializable
enum class ReaderFit {
    @SerialName("width") WIDTH,
    @SerialName("height") HEIGHT,
    @SerialName("both") BOTH,
    @SerialName("natural") NATURAL,
}

@Serializable
enum class ReaderDirection {
    @SerialName("ltr") LTR,
    @SerialName("rtl") RTL,
}

/** Settings › Lector (`AppSettings.reader`), classic ReaderPanel. Keymap keys are action ids private to :feature:manga. */
@Serializable
data class ReaderSettings(
    val mode: ReaderMode = ReaderMode.PAGED,
    val fit: ReaderFit = ReaderFit.WIDTH,
    val direction: ReaderDirection = ReaderDirection.RTL,
    val doublePage: Boolean = false,
    /** 0 = no limit (dp). */
    val maxWidthDp: Int = 0,
    val pageGapDp: Int = 0,
    /** `black` | `white` | `gray` | `theme`. */
    val background: String = "black",
    /** -1 = system brightness; else 0..1. */
    val brightness: Float = -1f,
    val cropBorders: Boolean = false,
    /** Eager (whole chapter) vs lazy (around the current page). */
    val eagerLoading: Boolean = false,
    /** Pages to prefetch ahead in lazy mode, 1..20. */
    val prefetchPages: Int = 5,
    /** Keep scrolling into the next chapter (vertical / webtoon). */
    val infiniteScroll: Boolean = true,
    /** 0 = off; else px per second. */
    val autoScrollSpeed: Int = 0,
    val fullscreenOnOpen: Boolean = true,
    val hideChromeOnTap: Boolean = true,
    val showProgress: Boolean = true,
    /** Zoom with volume keys / Ctrl+wheel on keyboards. */
    val zoomShortcuts: Boolean = true,
    /** Reader action id -> key name; empty = defaults of :feature:manga. */
    val keymap: Map<String, String> = emptyMap(),
) {
    fun sanitized(): ReaderSettings = copy(
        maxWidthDp = maxWidthDp.coerceIn(0, 4000),
        pageGapDp = pageGapDp.coerceIn(0, 64),
        background = background.takeIf { it in BACKGROUNDS } ?: "black",
        brightness = if (brightness.isFinite() && brightness >= 0f) brightness.coerceIn(0f, 1f) else -1f,
        prefetchPages = prefetchPages.coerceIn(1, 20),
        autoScrollSpeed = autoScrollSpeed.coerceIn(0, 2000),
    )

    companion object {
        val BACKGROUNDS: Set<String> = setOf("black", "white", "gray", "theme")
    }
}

/**
 * `Route.MangaReader(args)`. [mediaId] / [title] identify the manga for progress; the chapter is addressed by its extension coordinates.
 * [offline] opens the downloaded pages without network. Also used by provider pages (checklist `ext.custom-providers.read`): a `read`
 * entry of a custom provider is just a chapter of that extension, so r navigates here with the same args.
 */
@Serializable
data class MangaReaderArgs(
    val mediaId: String,
    val title: String,
    val poster: String? = null,
    val extensionId: String,
    val sourceId: String,
    val itemId: String,
    val chapterId: String,
    val chapterNumber: Double? = null,
    val startPage: Int = 0,
    val offline: Boolean = false,
    /** AniList id when known (readlist progress sync). */
    val anilistId: Int? = null,
)

/** Home of manga (classic MangaHomePage). Each row is independent like Home's feeds. */
data class MangaHome(
    val hero: LoadState<List<Media>> = LoadState.Loading,
    val trending: LoadState<List<Media>> = LoadState.Loading,
    val popular: LoadState<List<Media>> = LoadState.Loading,
    val topRated: LoadState<List<Media>> = LoadState.Loading,
    val genres: List<String> = emptyList(),
    val lastRead: List<ReadingProgress> = emptyList(),
    /** Followed (READING) titles with new chapters since the last visit. */
    val followedUpdates: List<Pair<ReadEntry, MangaChapter>> = emptyList(),
    /** 18+ zone (only loaded after the gate is accepted and `showAdult` is on). */
    val adult: LoadState<List<Media>>? = null,
)

/** Implemented by worker s (`MangaRepositoryImpl`, :core:manga). */
interface MangaRepository {
    fun home(): Flow<MangaHome>

    suspend fun refreshHome()

    /** `MetadataRepository.searchAdvanced(MANGA, query)`; [query] carries origin / status / genre / year / sort. */
    suspend fun search(query: MediaSearchQuery, page: Int = 1): FeedPage

    suspend fun details(ref: MediaRef): MangaDetails?

    /** Installed manga sources (custom `read` sources) matched to this title, best first (preferred language, reliability). */
    suspend fun chapterSources(ref: MediaRef, title: String): List<MangaChapterSource>

    /** Chapters of one source for this title (cached; [refresh] bypasses the cache). */
    suspend fun chapters(source: MangaChapterSource, refresh: Boolean = false): List<MangaChapter>

    /** Picks the best source for a chapter number according to language and reliability (classic auto-best). */
    suspend fun bestChapter(ref: MediaRef, title: String, number: Double): MangaChapter?

    suspend fun pages(chapter: MangaChapter): List<ChapterPage>

    suspend fun clearChapterCache(ref: MediaRef)

    // --- progress / readlist ---
    fun lastRead(limit: Int = 20): Flow<List<ReadingProgress>>

    fun progress(mediaId: String): Flow<ReadingProgress?>

    suspend fun saveProgress(progress: ReadingProgress)

    fun readlist(status: ListStatus? = null): Flow<List<ReadEntry>>

    fun readEntry(ref: MediaRef): Flow<ReadEntry?>

    suspend fun setReadStatus(media: Media, status: ListStatus?): Result<Unit>

    suspend fun setReadProgress(ref: MediaRef, chapters: Int): Result<Unit>

    // --- downloads ---
    suspend fun downloadChapters(media: Media, chapters: List<MangaChapter>): Result<Int>

    fun downloadedChapters(mediaId: String): Flow<List<MangaChapter>>

    suspend fun removeDownloadedChapter(chapterId: String)

    /** Pages of a downloaded chapter (`file://` URIs). */
    suspend fun offlinePages(chapterId: String): List<ChapterPage>
}
