// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.Serializable

// Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.1): Buscar + Ficha extras of the classic app
// (desktop workers a / c, Android worker o). AniList vocabulary is kept as provider strings (`"TV"`, `"RELEASING"`, `"WINTER"`,
// `"SEQUEL"`) because the legacy app, desktop and the AniList API all speak it. Signatures are FINAL; bodies: worker o (:core:metadata).

/** AniList `MediaSort` subset the search UI offers. */
@Serializable
enum class MediaSort { POPULARITY, TRENDING, SCORE, START_DATE, TITLE, FAVOURITES, EPISODES, RELEVANCE }

/**
 * Advanced search (legacy `SearchPage.tsx` anime filters). Every filter is optional; an empty query with filters browses.
 * [formats] = AniList `MediaFormat` (`TV`, `TV_SHORT`, `MOVIE`, `OVA`, `ONA`, `SPECIAL`, `MUSIC`); [status] = `MediaStatus`;
 * [season] = `MediaSeason`; [tags] = AniList tag names; [scoreFrom]/[scoreTo] 0..100; [adult] = include 18+ (default false).
 */
@Serializable
data class MediaSearchQuery(
    val query: String? = null,
    val genres: List<String> = emptyList(),
    val tags: List<String> = emptyList(),
    val formats: List<String> = emptyList(),
    val status: String? = null,
    val season: String? = null,
    val seasonYear: Int? = null,
    val yearFrom: Int? = null,
    val yearTo: Int? = null,
    val episodesFrom: Int? = null,
    val episodesTo: Int? = null,
    val scoreFrom: Int? = null,
    val scoreTo: Int? = null,
    val sort: MediaSort = MediaSort.POPULARITY,
    val adult: Boolean = false,
    /** Studio / producer name (`/search/producer/:name` of the classic app). */
    val studio: String? = null,
) {
    /** Nothing to search or browse. */
    val isEmpty: Boolean
        get() = query.isNullOrBlank() && genres.isEmpty() && tags.isEmpty() && formats.isEmpty() && status == null && season == null &&
            seasonYear == null && yearFrom == null && yearTo == null && episodesFrom == null && episodesTo == null &&
            scoreFrom == null && scoreTo == null && studio == null
}

/** AniList tag with its relevance percentage. Spoiler tags are hidden unless the user opts in. */
@Serializable
data class MediaTag(val name: String, val rank: Int? = null, val isSpoiler: Boolean = false, val category: String? = null)

/** YouTube / Dailymotion trailer. */
@Serializable
data class Trailer(val site: String, val id: String, val thumbnail: String? = null) {
    val url: String
        get() = when (site.lowercase()) {
            "youtube" -> "https://www.youtube.com/watch?v=$id"
            "dailymotion" -> "https://www.dailymotion.com/video/$id"
            else -> id
        }
}

@Serializable
data class NextAiring(val episode: Int, val airingAtEpochSec: Long)

/** The "ficha técnica" block of Details (classic `AnimeDetailLayout` facts) that [Media] does not carry. */
@Serializable
data class MediaDetails(
    val ref: MediaRef,
    val format: String? = null,
    /** AniList `source`: `MANGA`, `LIGHT_NOVEL`, `ORIGINAL`, ... */
    val source: String? = null,
    val durationMin: Int? = null,
    /** ISO-8601 dates, possibly partial (`2026-01`). */
    val startDate: String? = null,
    val endDate: String? = null,
    val meanScore: Int? = null,
    val popularity: Int? = null,
    val favourites: Int? = null,
    val studios: List<String> = emptyList(),
    val producers: List<String> = emptyList(),
    val countryOfOrigin: String? = null,
    val tags: List<MediaTag> = emptyList(),
    val synonyms: List<String> = emptyList(),
    val trailer: Trailer? = null,
    val nextAiring: NextAiring? = null,
    val isAdult: Boolean = false,
    /** Official streaming / external links as `label -> url`. */
    val externalLinks: Map<String, String> = emptyMap(),
)

/** One edge of AniList `relations` ([relation] = `MediaRelation`: `SEQUEL`, `PREQUEL`, `SIDE_STORY`, `ADAPTATION`, ...). */
@Serializable
data class MediaRelation(val relation: String, val media: Media)

@Serializable
data class CharacterRef(val id: Int, val name: String, val image: String? = null)

/** Staff or voice actor. [language] is the voice actor language (`JAPANESE`, `SPANISH`...), null for staff. */
@Serializable
data class StaffRef(val id: Int, val name: String, val image: String? = null, val language: String? = null)

@Serializable
data class CharacterEdge(val character: CharacterRef, val role: String? = null, val voiceActors: List<StaffRef> = emptyList())

@Serializable
data class StaffEdge(val staff: StaffRef, val role: String? = null)

/** AniList recommendation ([rating] = community votes). */
@Serializable
data class Recommendation(val media: Media, val rating: Int = 0)

/** One page of AniList character / staff edges. */
@Serializable
data class CharactersPage(val items: List<CharacterEdge>, val page: Int, val hasNext: Boolean)

@Serializable
data class CharacterAppearance(val media: Media, val role: String? = null)

/** Character page (classic `/character/:name`). */
@Serializable
data class Character(
    val id: Int,
    val name: String,
    val nativeName: String? = null,
    val image: String? = null,
    val description: String? = null,
    val age: String? = null,
    val birthday: String? = null,
    val bloodType: String? = null,
    val gender: String? = null,
    val favourites: Int? = null,
    val appearances: List<CharacterAppearance> = emptyList(),
    val voiceActors: List<StaffRef> = emptyList(),
)

/** One trace.moe hit (classic image search). Times are seconds into the episode; [similarity] 0..1. */
data class ImageMatch(
    val ref: MediaRef,
    val title: String,
    val episode: Int?,
    val fromSec: Double,
    val toSec: Double,
    val similarity: Double,
    val previewImage: String?,
    val previewVideo: String?,
)
