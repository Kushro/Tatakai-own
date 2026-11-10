// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import java.time.Instant
import kotlin.math.floor

/**
 * How the list progress (P episodes / chapters of T, status S, from the LOCAL list mirror the AniList / MAL sync maintains) joins the
 * local watch / read history. Shared semantics with desktop (`tk-core`): keep both identical.
 *
 * - Continue watching / reading = the history rows plus the list entries with S in {watching, repeating}, P > 0 and P < T (or T unknown);
 *   one item per title, episode = max(history episode, P + 1), the history position only when the history episode is the one shown.
 * - Details resume = max(history target, P + 1 when P > 0), capped to T; P >= T keeps the history behaviour (finished series).
 * - Episode n <= P counts as watched; the manual toggle sets P = max(P, n) (watched) or P = min(P, n - 1) (not watched).
 */
object ListProgress {

    /** The list entry still has something to watch / read next (S in {watching, repeating} and P < T or T unknown). */
    fun isContinuing(status: ListStatus?, progress: Int, total: Int?): Boolean =
        (status == ListStatus.WATCHING || status == ListStatus.REPEATING) && (total == null || total <= 0 || progress < total)

    /**
     * Continue watching: [history] are the existing history-based rows (one per anime, newest first); [list] the local list mirror.
     * List-only items are synthetic [HistoryEntry] rows with `episode = P + 1`, no position and no duration (no time bar), dated by
     * the list's `updatedAtMs`. Newest first by max(history watchedAt, list updatedAt); at most [limit].
     */
    fun mergeContinueWatching(history: List<HistoryEntry>, list: List<ListEntry>, limit: Int): List<HistoryEntry> {
        val continuing = list.filter { isContinuing(it.status, it.progress, it.totalEpisodes) }
        val used = HashSet<Int>()
        val out = ArrayList<Pair<HistoryEntry, Long>>()
        for (h in history) {
            val index = continuing.indexOfFirst { sameTitle(h, it.ref) }
            val l = if (index >= 0 && index !in used) continuing[index] else null
            if (index >= 0) used += index
            if (l == null) {
                out += h to h.watchedAt.toEpochMilli()
                continue
            }
            val sortKey = maxOf(h.watchedAt.toEpochMilli(), l.updatedAtMs)
            val listNext = l.progress + 1
            val item = if (listNext > h.episode) {
                h.copy(
                    episodeId = HistoryEntry.episodeIdOf(h.animeId, listNext),
                    episode = listNext,
                    progressSec = 0,
                    durationSec = null,
                    completed = false,
                    watchedAt = Instant.ofEpochMilli(sortKey),
                )
            } else {
                h.copy(watchedAt = Instant.ofEpochMilli(sortKey))
            }
            out += item to sortKey
        }
        continuing.forEachIndexed { i, l ->
            // list-only items only once started (P > 0): "watching" entries at 0 are a backlog, not something to continue
            if (i in used || l.progress <= 0) return@forEachIndexed
            val animeId = HistoryEntry.animeIdOf(l.ref.anilistId, l.ref.malId) ?: return@forEachIndexed
            val next = l.progress + 1
            out += HistoryEntry(
                animeId = animeId,
                title = l.title,
                poster = l.poster,
                episodeId = HistoryEntry.episodeIdOf(animeId, next),
                episode = next,
                progressSec = 0,
                durationSec = null,
                completed = false,
                watchedAt = Instant.ofEpochMilli(l.updatedAtMs),
                anilistId = l.ref.anilistId,
                malId = l.ref.malId,
            ) to l.updatedAtMs
        }
        return out.sortedByDescending { it.second }.take(limit.coerceAtLeast(0)).map { it.first }
    }

    /**
     * Details primary button: [historyTarget] is today's history rule (last unfinished episode, else the next one, 1 when nothing).
     * List progress [progress] > 0 moves it to max(historyTarget, P + 1) capped to [total]; P >= T keeps [historyTarget].
     */
    fun resumeEpisode(historyTarget: Int, progress: Int, total: Int?): Int {
        if (progress <= 0) return historyTarget
        val t = total?.takeIf { it > 0 }
        if (t != null && progress >= t) return historyTarget
        val e = maxOf(historyTarget, progress + 1)
        return if (t != null) minOf(e, t) else e
    }

    /** Episode [episode] is watched: within the list progress, or its history row is completed. */
    fun isEpisodeWatched(episode: Int, listProgress: Int, history: HistoryEntry?): Boolean =
        (episode in 1..listProgress) || history?.completed == true

    /** Manual toggle: the new list progress after marking [episode] watched / not watched. Never above [total] when known. */
    fun toggledProgress(current: Int, episode: Int, watched: Boolean, total: Int? = null): Int {
        val p = if (watched) maxOf(current, episode) else minOf(current, episode - 1)
        val t = total?.takeIf { it > 0 }
        return (if (t != null) minOf(p, t) else p).coerceAtLeast(0)
    }

    /**
     * Status to set before lowering the progress with "Marcar como no visto / no leído": a COMPLETED entry goes back to WATCHING
     * (Leyendo for read lists), like desktop; null = keep the status.
     */
    fun statusAfterUnmark(status: ListStatus?): ListStatus? = if (status == ListStatus.COMPLETED) ListStatus.WATCHING else null

    // ---------------------------------------------------------------- read lists (manga, novels): chapters

    /** Integer chapter a (possibly decimal) chapter number belongs to (10.5 -> 10); null when unknown. */
    fun chapterIndex(number: Double?): Int? = number?.takeIf { it.isFinite() && it >= 0 }?.let { floor(it).toInt() }

    /** Chapter row mark: its integer chapter is within the list progress. */
    fun isChapterRead(number: Double?, listProgress: Int): Boolean {
        val k = chapterIndex(number) ?: return false
        return k in 1..listProgress
    }

    /**
     * Details "Leer capítulo N": the saved reading position wins when it is unfinished and not behind the list ([resumeSaved]);
     * otherwise the next chapter after max(P, saved chapter).
     */
    fun resumeSaved(savedChapter: Double?, savedCompleted: Boolean, listProgress: Int): Boolean {
        if (savedCompleted) return false
        val k = chapterIndex(savedChapter) ?: return listProgress <= 0
        return k >= listProgress + 1 || listProgress <= 0
    }

    /** Chapters read through (the next chapter to open is the first one after this): max(P, saved chapter). */
    fun readThrough(savedChapter: Double?, listProgress: Int): Double = maxOf(listProgress.toDouble(), savedChapter ?: 0.0)

    /**
     * Continue reading: [positions] are the saved reading positions (one per title, any order); [list] the readlist. Titles whose list
     * entry is continuing show chapter max(saved, P + 1); [ContinueReadingItem.resume] = the saved position is the one shown.
     */
    fun mergeContinueReading(positions: List<ReadPosition>, list: List<ReadEntry>, limit: Int): List<ContinueReadingItem> {
        val continuing = list.filter { isContinuing(it.status, it.progress, it.totalChapters) }
        val used = HashSet<Int>()
        val out = ArrayList<ContinueReadingItem>()
        for (p in positions) {
            val index = continuing.indexOfFirst { sameRef(p.ref, it.ref) }
            val l = if (index >= 0 && index !in used) continuing[index] else null
            if (index >= 0) used += index
            if (l == null) {
                out += ContinueReadingItem(p.mediaId, p.ref, p.title, p.poster, p.chapter, resume = true, updatedAtMs = p.updatedAtMs)
                continue
            }
            val sortKey = maxOf(p.updatedAtMs, l.updatedAtMs)
            val saved = resumeSaved(p.chapter, p.completed, l.progress)
            out += ContinueReadingItem(
                mediaId = p.mediaId,
                ref = p.ref,
                title = p.title,
                poster = p.poster ?: l.poster,
                chapter = if (saved) p.chapter else (l.progress + 1).toDouble(),
                resume = saved,
                updatedAtMs = sortKey,
            )
        }
        continuing.forEachIndexed { i, l ->
            // list-only items only once started (P > 0): "watching" entries at 0 are a backlog, not something to continue
            if (i in used || l.progress <= 0) return@forEachIndexed
            val mediaId = HistoryEntry.animeIdOf(l.ref.anilistId, l.ref.malId) ?: return@forEachIndexed
            out += ContinueReadingItem(mediaId, l.ref, l.title, l.poster, (l.progress + 1).toDouble(), resume = false, updatedAtMs = l.updatedAtMs)
        }
        return out.sortedByDescending { it.updatedAtMs }.take(limit.coerceAtLeast(0))
    }

    private fun sameTitle(h: HistoryEntry, ref: MediaRef): Boolean =
        (h.anilistId != null && h.anilistId == ref.anilistId) ||
            (h.malId != null && h.malId == ref.malId) ||
            (ref.anilistId != null && h.animeId == ref.anilistId.toString()) ||
            (ref.malId != null && h.animeId == "mal:${ref.malId}")

    private fun sameRef(a: MediaRef, b: MediaRef): Boolean =
        (a.anilistId != null && a.anilistId == b.anilistId) || (a.malId != null && a.malId == b.malId)
}

/** A saved reading position (manga page / novel paragraph) reduced to what [ListProgress.mergeContinueReading] needs. */
data class ReadPosition(
    val mediaId: String,
    val ref: MediaRef,
    val title: String,
    val poster: String?,
    val chapter: Double?,
    val completed: Boolean,
    val updatedAtMs: Long,
)

/** One "Seguir leyendo" card: [resume] = open the saved position; else open the title's details at [chapter]. */
data class ContinueReadingItem(
    val mediaId: String,
    val ref: MediaRef,
    val title: String,
    val poster: String?,
    val chapter: Double?,
    val resume: Boolean,
    val updatedAtMs: Long,
) {
    /** Minimal [Media] to open the details of a list-only card (kind MANGA; novels are AniList MANGA with format NOVEL). */
    fun toMedia(): Media = Media(
        ref = MediaRef(anilistId = ref.anilistId, malId = ref.malId), kind = MediaKind.MANGA, title = title, titles = emptyMap(),
        cover = poster, banner = null, color = null, synopsis = null, genres = emptyList(), episodes = null, status = null, season = null,
        year = null, score = null, sourceProvider = "readlist",
    )
}

/**
 * Pulls a read list (manga / novels) from the linked AniList account. Bound by :core:manga and :core:novel so "Sincronizar ahora"
 * ([ListsRepository.syncNow]) refreshes them on every call. Returns the number of entries added or updated locally.
 */
fun interface ReadListPuller {
    suspend fun pullReadList(): Result<Int>
}
