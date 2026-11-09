// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import app.tatakai.next.model.CharacterEdge
import app.tatakai.next.model.Episode
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.NextAiring
import app.tatakai.next.model.StaffRef
import app.tatakai.next.model.TitleLanguage
import java.time.Instant

// Parity wave A (15 §5 o.2): pure helpers of the Details extras, unit-tested in DetailsLogicTest.

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

/** Countdown parts of the next airing episode; null once it aired. */
data class Countdown(val days: Long, val hours: Long, val minutes: Long)

fun countdown(airingAtEpochSec: Long, nowSec: Long): Countdown? {
    val left = airingAtEpochSec - nowSec
    if (left <= 0) return null
    return Countdown(days = left / 86_400, hours = (left % 86_400) / 3_600, minutes = (left % 3_600) / 60)
}

/** The title in [language] (`Media.titles` keys romaji / english / native), falling back to the provider's main title. */
internal fun Media.displayTitle(language: TitleLanguage): String = titles[language.key]?.takeIf { it.isNotBlank() } ?: title

/** A secondary line that differs from the [shown] title. */
internal fun Media.alternativeTitle(shown: String): String? =
    listOf("romaji", "native", "english").firstNotNullOfOrNull { key -> titles[key]?.takeIf { it.isNotBlank() && it != shown } }

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
