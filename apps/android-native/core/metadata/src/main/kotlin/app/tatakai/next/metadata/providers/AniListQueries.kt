// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata.providers

import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Parity wave A (worker o): AniList GraphQL documents of the classic Search / Details / Character pages (legacy
 * `src/lib/anilist/media.ts`, `src/pages/base/SearchPage.tsx`, `CharacterPage.tsx`) and the variable builder of the advanced search.
 */
internal object AniListQueries {
    private const val CARD = """
fragment mediaCard on Media {
  id idMal type status season seasonYear episodes
  title { romaji english native }
  coverImage { extraLarge large color } bannerImage
  averageScore isAdult genres
  description(asHtml: false)
}"""

    private const val VA_FIELDS = """
      vaJa: voiceActors(language: JAPANESE, sort: [RELEVANCE, ID]) { id name { full native } image { large medium } }
      vaEn: voiceActors(language: ENGLISH, sort: [RELEVANCE, ID]) { id name { full native } image { large medium } }
      vaEs: voiceActors(language: SPANISH, sort: [RELEVANCE, ID]) { id name { full native } image { large medium } }
      vaPt: voiceActors(language: PORTUGUESE, sort: [RELEVANCE, ID]) { id name { full native } image { large medium } }
      vaKo: voiceActors(language: KOREAN, sort: [RELEVANCE, ID]) { id name { full native } image { large medium } }"""

    /** Every filter of the classic SearchPage, all optional (AniList ignores absent variables). */
    const val ADVANCED_SEARCH = """
query (${'$'}page: Int, ${'$'}perPage: Int, ${'$'}type: MediaType, ${'$'}sort: [MediaSort], ${'$'}search: String, ${'$'}genre_in: [String],
  ${'$'}tag_in: [String], ${'$'}format_in: [MediaFormat], ${'$'}format_not_in: [MediaFormat], ${'$'}status: MediaStatus, ${'$'}season: MediaSeason,
  ${'$'}seasonYear: Int, ${'$'}startDate_greater: FuzzyDateInt, ${'$'}startDate_lesser: FuzzyDateInt, ${'$'}episodes_greater: Int,
  ${'$'}episodes_lesser: Int, ${'$'}averageScore_greater: Int, ${'$'}averageScore_lesser: Int, ${'$'}isAdult: Boolean) {
  Page(page: ${'$'}page, perPage: ${'$'}perPage) {
    pageInfo { total currentPage lastPage hasNextPage perPage }
    media(type: ${'$'}type, sort: ${'$'}sort, search: ${'$'}search, genre_in: ${'$'}genre_in, tag_in: ${'$'}tag_in, format_in: ${'$'}format_in,
      format_not_in: ${'$'}format_not_in, status: ${'$'}status, season: ${'$'}season, seasonYear: ${'$'}seasonYear,
      startDate_greater: ${'$'}startDate_greater, startDate_lesser: ${'$'}startDate_lesser, episodes_greater: ${'$'}episodes_greater,
      episodes_lesser: ${'$'}episodes_lesser, averageScore_greater: ${'$'}averageScore_greater, averageScore_lesser: ${'$'}averageScore_lesser,
      isAdult: ${'$'}isAdult) {
      ...mediaCard
    }
  }
}
$CARD"""

    /** Classic `/search/producer/:name`: the best-matching studio and its catalogue, most popular first. */
    const val STUDIO_MEDIA = """
query (${'$'}search: String, ${'$'}page: Int, ${'$'}perPage: Int) {
  Studio(search: ${'$'}search, sort: [SEARCH_MATCH]) {
    id name
    media(page: ${'$'}page, perPage: ${'$'}perPage, sort: [POPULARITY_DESC]) {
      pageInfo { currentPage hasNextPage }
      nodes { ...mediaCard }
    }
  }
}
$CARD"""

    /** Facts, trailer, next airing, relations and recommendations of one title in one request. */
    const val DETAILS = """
query (${'$'}id: Int, ${'$'}idMal: Int, ${'$'}type: MediaType) {
  Media(id: ${'$'}id, idMal: ${'$'}idMal, type: ${'$'}type) {
    id idMal format source duration countryOfOrigin isAdult synonyms meanScore popularity favourites
    startDate { year month day }
    endDate { year month day }
    studios { edges { isMain node { name isAnimationStudio } } }
    tags { name rank isMediaSpoiler isGeneralSpoiler category }
    trailer { id site thumbnail }
    nextAiringEpisode { episode airingAt }
    externalLinks { site url type }
    relations { edges { relationType(version: 2) node { ...mediaCard } } }
    recommendations(sort: [RATING_DESC], perPage: 15) { nodes { rating mediaRecommendation { ...mediaCard } } }
  }
}
$CARD"""

    /** One hop of the watch order walk: only the relations. */
    const val RELATIONS = """
query (${'$'}id: Int) {
  Media(id: ${'$'}id, type: ANIME) {
    id
    relations { edges { relationType(version: 2) node { ...mediaCard format startDate { year month day } } } }
  }
}
$CARD"""

    const val CHARACTERS = """
query (${'$'}id: Int, ${'$'}page: Int, ${'$'}perPage: Int) {
  Media(id: ${'$'}id) {
    id
    characters(page: ${'$'}page, perPage: ${'$'}perPage, sort: [ROLE, RELEVANCE, ID]) {
      pageInfo { currentPage hasNextPage }
      edges {
        role
        node { id name { full native } image { large medium } }
        $VA_FIELDS
      }
    }
  }
}"""

    const val STAFF = """
query (${'$'}id: Int, ${'$'}perPage: Int) {
  Media(id: ${'$'}id) {
    id
    staff(perPage: ${'$'}perPage, sort: [RELEVANCE, ID]) {
      edges { role node { id name { full native } image { large medium } } }
    }
  }
}"""

    const val CHARACTER = """
query (${'$'}id: Int) {
  Character(id: ${'$'}id) {
    id
    name { full native }
    image { large medium }
    description(asHtml: false)
    age gender bloodType favourites
    dateOfBirth { year month day }
    media(sort: [POPULARITY_DESC], perPage: 25, type: ANIME) {
      edges {
        characterRole
        node { ...mediaCard }
        $VA_FIELDS
      }
    }
  }
}
$CARD"""

    const val CHARACTER_SEARCH = """
query (${'$'}search: String, ${'$'}page: Int, ${'$'}perPage: Int) {
  Page(page: ${'$'}page, perPage: ${'$'}perPage) {
    characters(search: ${'$'}search, sort: [SEARCH_MATCH]) { id name { full native } image { large medium } }
  }
}"""

    const val GENRES = "query { GenreCollection }"

    const val TAGS = "query { MediaTagCollection { name category isGeneralSpoiler isAdult } }"

    /** Per-day trend rows since [date_greater] (unix seconds), best first; summed per media by `DiscoverMappers.trendsPage`. */
    const val TRENDS = """
query (${'$'}page: Int, ${'$'}perPage: Int, ${'$'}date_greater: Int) {
  Page(page: ${'$'}page, perPage: ${'$'}perPage) {
    pageInfo { currentPage hasNextPage }
    mediaTrends(date_greater: ${'$'}date_greater, sort: [TRENDING_DESC]) {
      mediaId trending
      media { ...mediaCard }
    }
  }
}
$CARD"""

    private fun strings(values: List<String>) = JsonArray(values.map { JsonPrimitive(it) })

    /** AniList `MediaSort` of [sort]; RELEVANCE (`SEARCH_MATCH`) is only valid with a text query, else popularity. */
    fun sortValue(sort: MediaSort, hasText: Boolean): String = when (sort) {
        MediaSort.POPULARITY -> "POPULARITY_DESC"
        MediaSort.TRENDING -> "TRENDING_DESC"
        MediaSort.SCORE -> "SCORE_DESC"
        MediaSort.START_DATE -> "START_DATE_DESC"
        MediaSort.TITLE -> "TITLE_ROMAJI"
        MediaSort.FAVOURITES -> "FAVOURITES_DESC"
        MediaSort.EPISODES -> "EPISODES_DESC"
        MediaSort.RELEVANCE -> if (hasText) "SEARCH_MATCH" else "POPULARITY_DESC"
    }

    /**
     * Variables of [ADVANCED_SEARCH]. AniList ranges are exclusive, so inclusive bounds are widened by one: years become
     * `startDate_greater = from*10000 - 1` (a year-only `from0000` date still matches) and `startDate_lesser = (to+1)*10000`; episodes and
     * score use `from-1` / `to+1`. 18+ off sends `isAdult: false`; on sends nothing (both). Seasons need a year: without
     * [MediaSearchQuery.seasonYear] the [currentYear] is used, like the classic filter.
     */
    fun advancedVariables(kind: MediaKind, q: MediaSearchQuery, page: Int, perPage: Int, currentYear: Int): JsonObject {
        val text = q.query?.trim().orEmpty()
        val vars = linkedMapOf<String, JsonElement>(
            "page" to JsonPrimitive(page.coerceAtLeast(1)),
            "perPage" to JsonPrimitive(perPage),
            "type" to JsonPrimitive(kind.name),
            "sort" to strings(listOf(sortValue(q.sort, text.isNotEmpty()))),
        )
        if (text.isNotEmpty()) vars["search"] = JsonPrimitive(text)
        if (q.genres.isNotEmpty()) vars["genre_in"] = strings(q.genres)
        if (q.tags.isNotEmpty()) vars["tag_in"] = strings(q.tags)
        if (q.formats.isNotEmpty()) vars["format_in"] = strings(q.formats.map { it.uppercase() })
        if (kind == MediaKind.MANGA && q.formats.isEmpty()) vars["format_not_in"] = strings(listOf("NOVEL"))
        q.status?.let { vars["status"] = JsonPrimitive(it.uppercase()) }
        q.season?.let {
            vars["season"] = JsonPrimitive(it.uppercase())
            vars["seasonYear"] = JsonPrimitive(q.seasonYear ?: currentYear)
        } ?: q.seasonYear?.let { vars["seasonYear"] = JsonPrimitive(it) }
        q.yearFrom?.let { vars["startDate_greater"] = JsonPrimitive(it * 10000 - 1) }
        q.yearTo?.let { vars["startDate_lesser"] = JsonPrimitive((it + 1) * 10000) }
        q.episodesFrom?.let { vars["episodes_greater"] = JsonPrimitive((it - 1).coerceAtLeast(0)) }
        q.episodesTo?.let { vars["episodes_lesser"] = JsonPrimitive(it + 1) }
        q.scoreFrom?.let { vars["averageScore_greater"] = JsonPrimitive((it - 1).coerceAtLeast(0)) }
        q.scoreTo?.let { vars["averageScore_lesser"] = JsonPrimitive(it + 1) }
        if (!q.adult) vars["isAdult"] = JsonPrimitive(false)
        return JsonObject(vars)
    }
}
