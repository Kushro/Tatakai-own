// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.Episode
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneOffset

/**
 * 0.3.2 (desktop `tk-core` episode merge): the first provider's list is the base; later lists (ani.zip first, then the other
 * providers) fill, by episode number, what the base lacks (title, image, air date, synopsis, length, rating) and add the numbers it
 * does not have when they already aired. Kitsu answers new shows with number-only rows ("Episode 3", no thumbnail): before this, the
 * router stopped there and never asked ani.zip, so every episode showed the series art and no synopsis (Overgeared, AniList 212888).
 */
object EpisodeMerge {
    private val GENERIC_TITLE = Regex(
        "^(episode|episodio|ep\\.?|eps?|第)\\s*\\d+\\s*(話|话)?$|^\\d+$",
        RegexOption.IGNORE_CASE,
    )

    /** A placeholder title ("Episode 3", "Episodio 3", "第3話", "3"): counts as missing. */
    fun isGenericTitle(title: String?): Boolean = title.isNullOrBlank() || GENERIC_TITLE.matches(title.trim())

    /** Missing fields worth asking another provider for, counted on aired (or undated) episodes only. */
    fun hasGaps(list: List<Episode>, nowMs: Long): Boolean = list.any { e ->
        aired(e, nowMs) && (isGenericTitle(e.title) || e.image.isNullOrBlank() || e.summary.isNullOrBlank())
    }

    /** True when [e] aired before [nowMs] or has no (readable) air date. */
    fun aired(e: Episode, nowMs: Long): Boolean = airedAtMs(e.airDate)?.let { it <= nowMs } ?: true

    /** `2023-09-29T15:00:00Z` / `2023-09-29` as epoch ms; null when absent or unreadable. */
    fun airedAtMs(airDate: String?): Long? {
        val s = airDate?.trim()?.takeIf { it.isNotEmpty() } ?: return null
        runCatching { return Instant.parse(s).toEpochMilli() }
        return runCatching { LocalDate.parse(s.take(10)).atStartOfDay(ZoneOffset.UTC).toInstant().toEpochMilli() }.getOrNull()
    }

    /**
     * [base] keeps its order of authority: each field it has stays; [extra] fills only missing ones (a generic title counts as
     * missing). Numbers only [extra] has are added when they aired by [nowMs] (or are undated within the base's range), so a
     * future-episode list never shows unaired rows as watchable.
     */
    fun merge(base: List<Episode>, extra: List<Episode>, nowMs: Long): List<Episode> {
        if (extra.isEmpty()) return base
        if (base.isEmpty()) return extra.filter { aired(it, nowMs) }.sortedBy { it.number }
        val byNumber = extra.associateBy { it.number }
        val filled = base.map { b -> byNumber[b.number]?.let { fill(b, it) } ?: b }
        val have = base.mapTo(HashSet()) { it.number }
        val maxBase = base.maxOf { it.number }
        val added = extra.filter { e ->
            e.number !in have && (airedAtMs(e.airDate)?.let { it <= nowMs } ?: (e.number <= maxBase))
        }
        return (filled + added).sortedBy { it.number }
    }

    /** [b] with its missing fields taken from [e] (same number). */
    fun fill(b: Episode, e: Episode): Episode = b.copy(
        title = b.title?.takeUnless { isGenericTitle(it) } ?: e.title?.takeUnless { isGenericTitle(it) } ?: b.title ?: e.title,
        image = b.image?.takeIf { it.isNotBlank() } ?: e.image,
        airDate = b.airDate?.takeIf { it.isNotBlank() } ?: e.airDate,
        isFiller = b.isFiller || e.isFiller,
        summary = b.summary?.takeIf { it.isNotBlank() } ?: e.summary,
        runtimeMin = b.runtimeMin ?: e.runtimeMin,
        rating = b.rating ?: e.rating,
    )
}
