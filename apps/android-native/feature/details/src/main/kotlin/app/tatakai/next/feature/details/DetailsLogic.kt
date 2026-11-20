// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import app.tatakai.next.model.CharacterEdge
import app.tatakai.next.model.Episode
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.ListProgress
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaArtwork
import app.tatakai.next.model.MediaDetails
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.NextAiring
import app.tatakai.next.model.StaffRef
import java.time.Instant

// Parity wave A (15 §5 o.2): pure helpers of the Details extras, unit-tested in DetailsLogicTest.

/** 0.3.2: the header backdrop: ani.zip fanart, else AniList banner, else cover; [lowRes] when it is not the fanart. */
data class HeaderBackdrop(val url: String?, val lowRes: Boolean)

fun headerBackdrop(media: Media, art: MediaArtwork?): HeaderBackdrop {
    art?.fanart?.takeIf { it.isNotBlank() }?.let { return HeaderBackdrop(it, lowRes = false) }
    return HeaderBackdrop(media.banner?.takeIf { it.isNotBlank() } ?: media.cover, lowRes = true)
}

/** One row of the episode list: a provider episode, or a placeholder for an announced one; [unaired] = not out yet. */
data class EpisodeItem(val episode: Episode, val unaired: Boolean)

/**
 * The provider's episodes plus placeholders up to the next airing one (`details.episodes.unaired`): an episode is unaired when its
 * air date is in the future or its number is >= [nextAiring]'s. Placeholders stop at the announced total when it is known.
 */
fun buildEpisodeItems(episodes: List<Episode>, nextAiring: NextAiring?, total: Int?, nowSec: Long): List<EpisodeItem> {
    val known = episodes.associateBy { it.number }
    val maxKnown = episodes.maxOfOrNull { it.number } ?: 0
    val nextEp = nextAiring?.episode
    val upTo = when {
        nextEp == null -> maxKnown
        total != null -> maxOf(maxKnown, minOf(nextEp, total))
        else -> maxOf(maxKnown, nextEp)
    }
    val numbers = (known.keys + (1..upTo)).distinct().sorted()
    return numbers.map { n ->
        val ep = known[n] ?: Episode(number = n, title = null, image = null, airDate = null)
        val futureDate = ep.airDate?.let { airEpochSec(it) }?.let { it > nowSec } == true
        EpisodeItem(ep, unaired = futureDate || (nextEp != null && n >= nextEp))
    }
}

private fun airEpochSec(raw: String): Long? =
    runCatching { Instant.parse(raw).epochSecond }.getOrNull()
        ?: runCatching { java.time.LocalDate.parse(raw.take(10)).atStartOfDay(java.time.ZoneOffset.UTC).toEpochSecond() }.getOrNull()

/** Episode search (`details.episodes.search`): a number matches that episode (and its prefix), text matches the title. */
fun filterEpisodes(items: List<EpisodeItem>, query: String): List<EpisodeItem> {
    val q = query.trim()
    if (q.isEmpty()) return items
    val number = q.toIntOrNull()
    return items.filter { item ->
        if (number != null) {
            item.episode.number.toString().startsWith(q)
        } else {
            item.episode.title?.contains(q, ignoreCase = true) == true
        }
    }
}

/**
 * Ficha "Primero el episodio": how many leading rows of [page] fold behind "Episodios anteriores (a–b)" so the list opens on the
 * episode to continue. Everything before `current - 1` folds (the previous episode stays as context), only when that is at least
 * [min] rows; 0 without a [current] episode or when it is not on this page.
 */
fun hiddenLeading(page: List<EpisodeItem>, current: Int?, min: Int = 3): Int {
    if (current == null || page.none { it.episode.number == current }) return 0
    val count = page.indexOfFirst { it.episode.number >= current - 1 }.takeIf { it >= 0 } ?: return 0
    return if (count >= min) count else 0
}

/** Countdown parts of the next airing episode; null once it aired. */
data class Countdown(val days: Long, val hours: Long, val minutes: Long)

fun countdown(airingAtEpochSec: Long, nowSec: Long): Countdown? {
    val left = airingAtEpochSec - nowSec
    if (left <= 0) return null
    return Countdown(days = left / 86_400, hours = (left % 86_400) / 3_600, minutes = (left % 3_600) / 60)
}

// Media.displayTitle / alternativeTitle moved up to :core:ui (app.tatakai.next.ui.media.TitleLanguage.kt).

/** AniList's adult flag is on [app.tatakai.next.model.MediaDetails]; before it loads, the Hentai genre marks the title. */
internal fun Media.looksAdult(): Boolean = genres.any { it.equals("Hentai", ignoreCase = true) }

/** Classic relation order: the story chain first, then adaptations and the rest, each group alphabetical by relation. */
private val RELATION_ORDER = listOf("PREQUEL", "SEQUEL", "PARENT", "SIDE_STORY", "SPIN_OFF", "ALTERNATIVE", "SUMMARY", "ADAPTATION", "SOURCE")

fun sortRelations(relations: List<MediaRelation>): List<MediaRelation> =
    relations.sortedWith(compareBy({ RELATION_ORDER.indexOf(it.relation).let { i -> if (i < 0) RELATION_ORDER.size else i } }, { it.relation }))

/** Japanese voice actor first (classic default), then the others. */
fun preferredVoiceActor(edge: CharacterEdge): StaffRef? =
    edge.voiceActors.firstOrNull { it.language.equals("JAPANESE", ignoreCase = true) } ?: edge.voiceActors.firstOrNull()

/** `https://anilist.co/anime/<id>` (classic ShareButton link) or the MAL page when only that id is known. */
fun shareLink(media: Media): String? =
    media.ref.anilistId?.let { "https://anilist.co/${if (media.kind == app.tatakai.next.model.MediaKind.ANIME) "anime" else "manga"}/$it" }
        ?: media.ref.malId?.let { "https://myanimelist.net/anime/$it" }

// ---------------------------------------------------------------- "Cabecera de la ficha" (LookPrefs.headerLayout)

/**
 * Where the resume position stands: [watchedSec] of [durationSec] (null = no length known, only "Retomar en 3:39"). [estimated] = the
 * length is the episode's or the title's runtime, not the player's own (shown as "quedan ~20 min").
 */
data class ResumeProgress(val watchedSec: Int, val durationSec: Int?, val estimated: Boolean = false) {
    val fraction: Float? get() = durationSec?.let { (watchedSec.toFloat() / it).coerceIn(0f, 1f) }
    val remainingSec: Int? get() = durationSec?.let { (it - watchedSec).coerceAtLeast(0) }

    /** Whole minutes of [durationSec], rounded. */
    val durationMin: Int? get() = durationSec?.let { roundedMinutes(it) }

    /** Whole minutes of [remainingSec], rounded (at least 1 while something is left). */
    val remainingMin: Int? get() = remainingSec?.let { if (it > 0) maxOf(1, roundedMinutes(it)) else 0 }
}

/** Seconds to whole minutes, rounded half up. */
fun roundedMinutes(sec: Int): Int = (sec + 30) / 60

/**
 * The resume position of [entry] with the best known length: the history's own duration, else the episode's runtime, else the
 * title's [fallbackMin]; a length that does not exceed the watched time is skipped. null without an entry.
 */
fun resumeProgress(entry: HistoryEntry?, episode: Episode?, fallbackMin: Int?): ResumeProgress? {
    entry ?: return null
    val watched = entry.progressSec
    val own = entry.durationSec?.takeIf { it > watched }
    if (own != null) return ResumeProgress(watched, own)
    val duration = sequenceOf(episode?.runtimeMin?.let { it * 60 }, fallbackMin?.let { it * 60 })
        .firstOrNull { it != null && it > watched }
    return ResumeProgress(watched, duration, estimated = duration != null)
}

/** Episodes watched by the list progress or a completed history row, within [total] when known. */
fun watchedCount(progress: Map<Int, HistoryEntry>, listProgress: Int, total: Int?): Int =
    (progress.keys + (1..listProgress)).distinct().count { n ->
        n >= 1 && (total == null || n <= total) && ListProgress.isEpisodeWatched(n, listProgress, progress[n])
    }

private val GENERIC_TITLE = Regex("""^\s*(episode|episodio|ep\.?|e|#)?\s*0*(\d+)\s*$""", RegexOption.IGNORE_CASE)

/** "Episode 4", "Episodio 04", "Ep. 4", "4": a provider title that only repeats the number (the card already says it). */
fun isGenericEpisodeTitle(title: String, number: Int): Boolean =
    GENERIC_TITLE.matchEntire(title)?.groupValues?.get(2)?.toIntOrNull() == number

/** State of the next-episode card / main button. [UPCOMING] = up to date before anything was watched ("Próximamente"). */
enum class NextUpKind { START, CONTINUE, REWATCH, UP_TO_DATE, UPCOMING }

/** What the header says about the episode to play: [episode] of [total], its [title] (null while the list loads), the resume point. */
data class NextUp(
    val kind: NextUpKind,
    val episode: Int,
    val total: Int?,
    val title: String?,
    val progress: ResumeProgress?,
    val airing: NextAiring?,
    val movie: Boolean,
) {
    val playable: Boolean get() = kind != NextUpKind.UP_TO_DATE && kind != NextUpKind.UPCOMING
}

fun DetailsUiState.nextUp(nowSec: Long): NextUp {
    val kind = when (watchActionAt(nowSec)) {
        WatchAction.START -> NextUpKind.START
        WatchAction.CONTINUE -> NextUpKind.CONTINUE
        WatchAction.REWATCH -> NextUpKind.REWATCH
        WatchAction.UP_TO_DATE -> if (hasProgress) NextUpKind.UP_TO_DATE else NextUpKind.UPCOMING
    }
    val details = detailsOrNull
    val media = (media as? LoadState.Ok)?.value
    val episode = continueEpisode
    val row = (episodes as? LoadState.Ok)?.value?.firstOrNull { it.number == episode }
    val total = totalEpisodes
    val format = details?.format ?: media?.format
    return NextUp(
        kind = kind,
        episode = episode,
        total = total,
        title = row?.title?.takeIf { it.isNotBlank() && !isGenericEpisodeTitle(it, episode) },
        progress = resumeProgress(resumeEntry, row, details?.durationMin),
        airing = details?.nextAiring,
        movie = total == 1 || format.equals("MOVIE", ignoreCase = true),
    )
}

/** One labelled fact of the header (grid cell or part of the facts line); a missing value is left out, never shown as a dash. */
sealed interface HeadFact {
    data class Status(val raw: String) : HeadFact
    data class Next(val episode: Int, val left: Countdown) : HeadFact
    data class Episodes(val watched: Int, val total: Int?) : HeadFact
    data class Year(val year: Int) : HeadFact
    data class Score(val score: Int) : HeadFact
}

private val KNOWN_STATUS = setOf("RELEASING", "FINISHED", "NOT_YET_RELEASED", "CANCELLED", "HIATUS")

/** Status, next airing (while counting down), episodes (not for a movie), year and score, in that order. */
fun headFacts(media: Media, details: MediaDetails?, watched: Int, total: Int?, nowSec: Long): List<HeadFact> = buildList {
    media.status?.uppercase()?.takeIf { it in KNOWN_STATUS }?.let { add(HeadFact.Status(it)) }
    details?.nextAiring?.let { next -> countdown(next.airingAtEpochSec, nowSec)?.let { add(HeadFact.Next(next.episode, it)) } }
    val movie = total == 1 || (details?.format ?: media.format).equals("MOVIE", ignoreCase = true)
    if (!movie) {
        when {
            total != null && total > 1 -> add(HeadFact.Episodes(watched, total))
            total == null && watched > 0 -> add(HeadFact.Episodes(watched, null))
        }
    }
    media.year?.let { add(HeadFact.Year(it)) }
    media.score?.let { add(HeadFact.Score(it)) }
}

/** How much the header may show for the window height: [TIGHT] drops the card's cover and times, [SHORT] also the second fact row. */
enum class HeadFit { FULL, TIGHT, SHORT }

fun headFit(heightDp: Int, fontScale: Float): HeadFit = when {
    heightDp < 700 -> HeadFit.SHORT
    heightDp < 840 || fontScale >= 1.15f -> HeadFit.TIGHT
    else -> HeadFit.FULL
}

/**
 * The facts a grid of [columns] shows in at most [maxRows] rows: when they do not fit, the score goes first, then the year, then
 * whatever is left at the end.
 */
fun gridFacts(facts: List<HeadFact>, columns: Int, maxRows: Int): List<HeadFact> {
    val max = if (maxRows == Int.MAX_VALUE) Int.MAX_VALUE else columns * maxRows
    var shown = facts
    if (shown.size > max) shown = shown.filterNot { it is HeadFact.Score }
    if (shown.size > max) shown = shown.filterNot { it is HeadFact.Year }
    return shown.take(max)
}

/** Rows the grid may use: 1 on short windows, 2 at a large font (2 columns x 3 rows does not fit at 800 dp), else all. */
fun gridMaxRows(fit: HeadFit, fontScale: Float): Int = when {
    fit == HeadFit.SHORT -> 1
    fontScale >= 1.3f -> 2
    else -> Int.MAX_VALUE
}

/** Columns of the facts grid: 2 at a large font or when a third of [widthDp] is under 96 dp. */
fun gridColumns(widthDp: Float, fontScale: Float): Int = if (fontScale >= 1.3f || widthDp / 3 < 96f) 2 else 3

/** Accessible name of a control: [long] when it contains the [visible] text (WCAG label in name), else the visible text itself. */
fun labelInName(visible: String, long: String): String = if (long.contains(visible)) long else visible

/** The first of [candidates] that [fits] (longest label first), or null when none does. */
fun <T> firstFitting(candidates: List<T>, fits: (T) -> Boolean): T? = candidates.firstOrNull(fits)
