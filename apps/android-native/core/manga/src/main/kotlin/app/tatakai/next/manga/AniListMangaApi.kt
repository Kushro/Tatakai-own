// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.CharacterEdge
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.MediaTag
import app.tatakai.next.model.Recommendation
import app.tatakai.next.model.StaffEdge
import app.tatakai.next.model.StaffRef
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

/** Everything the manga / novel details pages show from AniList (classic MangaPage / NovelPage). */
data class MangaInfo(
    val media: Media,
    /** AniList `MediaFormat`: `MANGA`, `ONE_SHOT`, `NOVEL`. */
    val format: String? = null,
    val chapters: Int? = null,
    val volumes: Int? = null,
    /** AniList `MediaStatus`. */
    val status: String? = null,
    /** `JP` / `KR` / `CN` / `TW`. */
    val origin: String? = null,
    val isAdult: Boolean = false,
    val source: String? = null,
    val meanScore: Int? = null,
    val popularity: Int? = null,
    val favourites: Int? = null,
    /** ISO-8601, possibly partial. */
    val startDate: String? = null,
    val endDate: String? = null,
    val synonyms: List<String> = emptyList(),
    val tags: List<MediaTag> = emptyList(),
    /** Story / art staff. */
    val authors: List<StaffEdge> = emptyList(),
    val characters: List<CharacterEdge> = emptyList(),
    val relations: List<MediaRelation> = emptyList(),
    val recommendations: List<Recommendation> = emptyList(),
    /** "#12 Most popular all time" style labels (AniList `rankings`). */
    val rankings: List<String> = emptyList(),
    val externalLinks: Map<String, String> = emptyMap(),
)

/** One row of the user's AniList manga list (`MediaListCollection(type: MANGA)`). */
data class AniListReadRow(
    val media: Media,
    val format: String?,
    val status: ListStatus,
    val progress: Int,
    val score: Int?,
    val updatedAtSec: Long?,
    val totalChapters: Int?,
)

/**
 * Private AniList GraphQL client of the readers (manga and light novels). :core:metadata only knows the ANIME shape of `Media` (its
 * details document is typed ANIME); the readers need chapters, volumes, `countryOfOrigin`, the publication status and the manga list
 * mutations, so they ask AniList directly (`graphql.anilist.co`, allowed host). Rate limit (HTTP 429) -> [AppError.RateLimited].
 */
class AniListMangaApi(
    private val client: OkHttpClient,
    private val endpoint: String = ENDPOINT,
) {
    /** Advanced search of manga ([novels] = false: every format but NOVEL) or light novels ([novels] = true). */
    suspend fun search(query: MediaSearchQuery, page: Int, novels: Boolean): FeedPage {
        val doc = post(SEARCH, searchVariables(query, page, PER_PAGE, novels))
        return parsePage(doc, page)
    }

    suspend fun info(ref: MediaRef): MangaInfo? {
        val anilistId = ref.anilistId
        val malId = ref.malId
        if (anilistId == null && malId == null) return null
        val vars = buildJsonObject {
            if (anilistId != null) put("id", anilistId) else put("idMal", malId)
        }
        val doc = try {
            post(DETAILS, vars)
        } catch (e: AppException) {
            if ((e.error as? AppError.Http)?.code == 404) return null else throw e
        }
        val m = doc.obj().sub("data").sub("Media") ?: return null
        if ((m.int("id") ?: 0) == 0) return null
        return parseInfo(m)
    }

    suspend fun genres(): List<String> {
        val doc = post("query { GenreCollection }", JsonObject(emptyMap()))
        return doc.obj().sub("data").strings("GenreCollection")
    }

    suspend fun viewerId(token: String): Int? = post(VIEWER, JsonObject(emptyMap()), token).obj().sub("data").sub("Viewer").int("id")

    /** The user's manga list (novels included; callers filter by [AniListReadRow.format]). */
    suspend fun readList(token: String): List<AniListReadRow> {
        val userId = viewerId(token) ?: return emptyList()
        val doc = post(LIST_COLLECTION, buildJsonObject { put("userId", userId) }, token)
        return parseReadList(doc)
    }

    /** `SaveMediaListEntry`: status and / or progress of one title. */
    suspend fun saveEntry(token: String, mediaId: Int, status: ListStatus?, progress: Int?) {
        val vars = buildJsonObject {
            put("mediaId", mediaId)
            status?.let { put("status", anilistStatus(it)) }
            progress?.let { put("progress", it) }
        }
        post(SAVE_ENTRY, vars, token)
    }

    /** Removes the title from the user's list (no-op when it is not there). */
    suspend fun deleteEntry(token: String, mediaId: Int) {
        val doc = post(ENTRY_ID, buildJsonObject { put("id", mediaId) }, token)
        val entryId = doc.obj().sub("data").sub("Media").sub("mediaListEntry").int("id") ?: return
        post(DELETE_ENTRY, buildJsonObject { put("id", entryId) }, token)
    }

    private suspend fun post(query: String, variables: JsonObject, token: String? = null): JsonElement = withContext(Dispatchers.IO) {
        val body = buildJsonObject {
            put("query", query)
            put("variables", variables)
        }.toString().toRequestBody(JSON)
        val request = Request.Builder()
            .url(endpoint)
            .header("Accept", "application/json")
            .apply { if (token != null) header("Authorization", "Bearer $token") }
            .post(body)
            .build()
        val text = try {
            client.newCall(request).execute().use { response ->
                if (response.code == 429) throw AppException(AppError.RateLimited("AniList"))
                val raw = response.body.string()
                if (!response.isSuccessful && raw.isBlank()) throw AppException(AppError.Http(response.code))
                if (response.code == 404) throw AppException(AppError.Http(404))
                if (!response.isSuccessful && response.code != 400) throw AppException(AppError.Http(response.code))
                raw
            }
        } catch (e: IOException) {
            throw AppException(AppError.Offline, e)
        }
        val doc = runCatching { ReaderJson.parseToJsonElement(text) }.getOrElse { throw AppException(AppError.Unknown("AniList: bad JSON"), it) }
        val errors = doc.obj().list("errors")
        if (errors.isNotEmpty() && doc.obj().sub("data").let { it == null || it.values.all { v -> v is kotlinx.serialization.json.JsonNull } }) {
            val status = errors.firstNotNullOfOrNull { it.int("status") }
            throw AppException(if (status != null) AppError.Http(status) else AppError.Unknown(errors.first().str("message") ?: "AniList"))
        }
        doc
    }

    companion object {
        const val ENDPOINT: String = "https://graphql.anilist.co"
        const val PER_PAGE: Int = 30
        private val JSON = "application/json; charset=utf-8".toMediaType()

        private const val CARD = """
fragment card on Media {
  id idMal type format status chapters volumes countryOfOrigin isAdult averageScore genres
  title { romaji english native }
  coverImage { extraLarge large color } bannerImage
  startDate { year month day }
  description(asHtml: false)
}"""

        const val SEARCH = """
query (${'$'}page: Int, ${'$'}perPage: Int, ${'$'}sort: [MediaSort], ${'$'}search: String, ${'$'}genre_in: [String], ${'$'}tag_in: [String],
  ${'$'}format_in: [MediaFormat], ${'$'}format_not_in: [MediaFormat], ${'$'}status: MediaStatus, ${'$'}countryOfOrigin: CountryCode,
  ${'$'}startDate_greater: FuzzyDateInt, ${'$'}startDate_lesser: FuzzyDateInt, ${'$'}averageScore_greater: Int, ${'$'}isAdult: Boolean) {
  Page(page: ${'$'}page, perPage: ${'$'}perPage) {
    pageInfo { currentPage hasNextPage }
    media(type: MANGA, sort: ${'$'}sort, search: ${'$'}search, genre_in: ${'$'}genre_in, tag_in: ${'$'}tag_in, format_in: ${'$'}format_in,
      format_not_in: ${'$'}format_not_in, status: ${'$'}status, countryOfOrigin: ${'$'}countryOfOrigin,
      startDate_greater: ${'$'}startDate_greater, startDate_lesser: ${'$'}startDate_lesser, averageScore_greater: ${'$'}averageScore_greater,
      isAdult: ${'$'}isAdult) { ...card }
  }
}
$CARD"""

        const val DETAILS = """
query (${'$'}id: Int, ${'$'}idMal: Int) {
  Media(id: ${'$'}id, idMal: ${'$'}idMal, type: MANGA) {
    ...card
    source meanScore popularity favourites synonyms
    endDate { year month day }
    tags { name rank isMediaSpoiler isGeneralSpoiler category }
    rankings { rank type allTime context year }
    staff(perPage: 8, sort: [RELEVANCE, ID]) { edges { role node { id name { full } image { medium } } } }
    characters(perPage: 16, sort: [ROLE, RELEVANCE, ID]) { edges { role node { id name { full } image { medium } } } }
    relations { edges { relationType(version: 2) node { ...card } } }
    recommendations(sort: [RATING_DESC], perPage: 15) { nodes { rating mediaRecommendation { ...card } } }
    externalLinks { site url }
  }
}
$CARD"""

        const val VIEWER = "query { Viewer { id } }"

        const val LIST_COLLECTION = """
query (${'$'}userId: Int) {
  MediaListCollection(userId: ${'$'}userId, type: MANGA) {
    lists { entries { status progress score(format: POINT_100) updatedAt media { ...card } } }
  }
}
$CARD"""

        const val SAVE_ENTRY = """
mutation (${'$'}mediaId: Int, ${'$'}status: MediaListStatus, ${'$'}progress: Int) {
  SaveMediaListEntry(mediaId: ${'$'}mediaId, status: ${'$'}status, progress: ${'$'}progress) { id status progress }
}"""

        const val ENTRY_ID = """query (${'$'}id: Int) { Media(id: ${'$'}id) { id mediaListEntry { id } } }"""

        const val DELETE_ENTRY = """mutation (${'$'}id: Int) { DeleteMediaListEntry(id: ${'$'}id) { deleted } }"""

        /** AniList `MediaListStatus` of a [ListStatus]. */
        fun anilistStatus(status: ListStatus): String = when (status) {
            ListStatus.WATCHING -> "CURRENT"
            ListStatus.PLANNING -> "PLANNING"
            ListStatus.COMPLETED -> "COMPLETED"
            ListStatus.PAUSED -> "PAUSED"
            ListStatus.DROPPED -> "DROPPED"
            ListStatus.REPEATING -> "REPEATING"
        }

        fun listStatusOf(anilist: String?): ListStatus? = when (anilist?.uppercase()) {
            "CURRENT" -> ListStatus.WATCHING
            "PLANNING" -> ListStatus.PLANNING
            "COMPLETED" -> ListStatus.COMPLETED
            "PAUSED" -> ListStatus.PAUSED
            "DROPPED" -> ListStatus.DROPPED
            "REPEATING" -> ListStatus.REPEATING
            else -> null
        }

        fun sortValue(sort: MediaSort, hasText: Boolean): String = when (sort) {
            MediaSort.POPULARITY -> "POPULARITY_DESC"
            MediaSort.TRENDING -> "TRENDING_DESC"
            MediaSort.SCORE -> "SCORE_DESC"
            MediaSort.START_DATE -> "START_DATE_DESC"
            MediaSort.TITLE -> "TITLE_ROMAJI"
            MediaSort.FAVOURITES -> "FAVOURITES_DESC"
            MediaSort.EPISODES -> "CHAPTERS_DESC"
            MediaSort.RELEVANCE -> if (hasText) "SEARCH_MATCH" else "POPULARITY_DESC"
        }

        /**
         * Variables of [SEARCH]. Year bounds are inclusive (widened by one because AniList ranges are exclusive), the origin is the AniList
         * `countryOfOrigin`, 18+ off sends `isAdult: false` and on sends `isAdult: true` (the 18+ zone asks only for adult titles).
         */
        fun searchVariables(q: MediaSearchQuery, page: Int, perPage: Int, novels: Boolean): JsonObject {
            val text = q.query?.trim().orEmpty()
            val vars = linkedMapOf<String, JsonElement>(
                "page" to JsonPrimitive(page.coerceAtLeast(1)),
                "perPage" to JsonPrimitive(perPage),
                "sort" to JsonArray(listOf(JsonPrimitive(sortValue(q.sort, text.isNotEmpty())))),
            )
            if (text.isNotEmpty()) vars["search"] = JsonPrimitive(text)
            if (q.genres.isNotEmpty()) vars["genre_in"] = JsonArray(q.genres.map { JsonPrimitive(it) })
            if (q.tags.isNotEmpty()) vars["tag_in"] = JsonArray(q.tags.map { JsonPrimitive(it) })
            if (novels) {
                vars["format_in"] = JsonArray(listOf(JsonPrimitive("NOVEL")))
            } else if (q.formats.isNotEmpty() && q.formats.none { it.equals("NOVEL", ignoreCase = true) }) {
                vars["format_in"] = JsonArray(q.formats.map { JsonPrimitive(it.uppercase()) })
            } else {
                vars["format_not_in"] = JsonArray(listOf(JsonPrimitive("NOVEL")))
            }
            q.status?.let { vars["status"] = JsonPrimitive(it.uppercase()) }
            q.origin?.takeIf { it.isNotBlank() }?.let { vars["countryOfOrigin"] = JsonPrimitive(it.uppercase()) }
            val from = q.yearFrom ?: q.seasonYear
            val to = q.yearTo ?: q.seasonYear
            from?.let { vars["startDate_greater"] = JsonPrimitive(it * 10000 - 1) }
            to?.let { vars["startDate_lesser"] = JsonPrimitive((it + 1) * 10000) }
            q.scoreFrom?.let { vars["averageScore_greater"] = JsonPrimitive((it - 1).coerceAtLeast(0)) }
            vars["isAdult"] = JsonPrimitive(q.adult)
            return JsonObject(vars)
        }

        fun parsePage(doc: JsonElement, page: Int): FeedPage {
            val p = doc.obj().sub("data").sub("Page")
            val items = p.list("media").filter { (it.int("id") ?: 0) != 0 }.map(::parseMedia)
            val info = p.sub("pageInfo")
            return FeedPage(items, info.int("currentPage") ?: page, info.bool("hasNextPage"))
        }

        /** A `card` fragment as a [Media] of kind MANGA ([Media.episodes] = chapters, [Media.year] = start year). */
        fun parseMedia(m: JsonObject): Media {
            val t = m.sub("title")
            val titles = linkedMapOf<String, String>()
            t.str("romaji")?.let { titles["romaji"] = it }
            t.str("english")?.let { titles["english"] = it }
            t.str("native")?.let { titles["native"] = it }
            val cover = m.sub("coverImage")
            return Media(
                ref = MediaRef(anilistId = m.int("id"), malId = m.int("idMal")),
                kind = if (m.str("type") == "ANIME") MediaKind.ANIME else MediaKind.MANGA,
                title = t.str("english") ?: t.str("romaji") ?: t.str("native") ?: "",
                titles = titles,
                cover = cover.str("extraLarge") ?: cover.str("large"),
                banner = m.str("bannerImage"),
                color = cover.str("color"),
                synopsis = cleanSynopsis(m.str("description")),
                genres = m.strings("genres"),
                episodes = m.int("chapters") ?: m.int("episodes"),
                status = m.str("status"),
                season = null,
                year = m.sub("startDate").int("year"),
                score = m.int("averageScore"),
                sourceProvider = "anilist",
            )
        }

        fun parseInfo(m: JsonObject): MangaInfo {
            val media = parseMedia(m)
            val tags = m.list("tags").mapNotNull { tag ->
                val name = tag.str("name") ?: return@mapNotNull null
                MediaTag(name, tag.int("rank"), tag.bool("isMediaSpoiler") || tag.bool("isGeneralSpoiler"), tag.str("category"))
            }
            val staff = m.sub("staff").list("edges").mapNotNull { e ->
                val n = e.sub("node") ?: return@mapNotNull null
                val id = n.int("id") ?: return@mapNotNull null
                StaffEdge(StaffRef(id, n.sub("name").str("full") ?: return@mapNotNull null, n.sub("image").str("medium")), e.str("role"))
            }
            val characters = m.sub("characters").list("edges").mapNotNull { e ->
                val n = e.sub("node") ?: return@mapNotNull null
                val id = n.int("id") ?: return@mapNotNull null
                CharacterEdge(CharacterRef(id, n.sub("name").str("full") ?: return@mapNotNull null, n.sub("image").str("medium")), e.str("role"))
            }
            val relations = m.sub("relations").list("edges").mapNotNull { e ->
                val node = e.sub("node") ?: return@mapNotNull null
                if ((node.int("id") ?: 0) == 0) return@mapNotNull null
                MediaRelation(e.str("relationType") ?: "OTHER", parseMedia(node))
            }
            val recs = m.sub("recommendations").list("nodes").mapNotNull { r ->
                val node = r.sub("mediaRecommendation") ?: return@mapNotNull null
                if ((node.int("id") ?: 0) == 0) return@mapNotNull null
                Recommendation(parseMedia(node), r.int("rating") ?: 0)
            }
            val rankings = m.list("rankings").mapNotNull { r ->
                val rank = r.int("rank") ?: return@mapNotNull null
                val context = r.str("context") ?: return@mapNotNull null
                val year = r.int("year")
                buildString {
                    append('#').append(rank).append(' ').append(context)
                    if (!r.bool("allTime") && year != null) append(' ').append(year)
                }
            }
            val links = m.list("externalLinks").mapNotNull { l ->
                val site = l.str("site") ?: return@mapNotNull null
                val url = l.str("url") ?: return@mapNotNull null
                site to url
            }.toMap()
            return MangaInfo(
                media = media,
                format = m.str("format"),
                chapters = m.int("chapters"),
                volumes = m.int("volumes"),
                status = m.str("status"),
                origin = m.str("countryOfOrigin"),
                isAdult = m.bool("isAdult"),
                source = m.str("source"),
                meanScore = m.int("meanScore"),
                popularity = m.int("popularity"),
                favourites = m.int("favourites"),
                startDate = fuzzyDate(m.sub("startDate")),
                endDate = fuzzyDate(m.sub("endDate")),
                synonyms = m.strings("synonyms"),
                tags = tags,
                authors = staff,
                characters = characters,
                relations = relations,
                recommendations = recs,
                rankings = rankings,
                externalLinks = links,
            )
        }

        fun parseReadList(doc: JsonElement): List<AniListReadRow> =
            doc.obj().sub("data").sub("MediaListCollection").list("lists").flatMap { it.list("entries") }.mapNotNull { e ->
                val m = e.sub("media") ?: return@mapNotNull null
                if ((m.int("id") ?: 0) == 0) return@mapNotNull null
                val status = listStatusOf(e.str("status")) ?: return@mapNotNull null
                AniListReadRow(
                    media = parseMedia(m),
                    format = m.str("format"),
                    status = status,
                    progress = e.int("progress") ?: 0,
                    score = e.int("score")?.takeIf { it > 0 },
                    updatedAtSec = e.long("updatedAt"),
                    totalChapters = m.int("chapters"),
                )
            }.distinctBy { it.media.ref.anilistId }

        private fun fuzzyDate(d: JsonObject?): String? {
            val y = d.int("year") ?: return null
            val mo = d.int("month")
            val day = d.int("day")
            return buildString {
                append(y)
                if (mo != null) {
                    append('-').append(mo.toString().padStart(2, '0'))
                    if (day != null) append('-').append(day.toString().padStart(2, '0'))
                }
            }
        }

        /** AniList descriptions carry `<br>` / `<i>` tags and "(Source: …)" notes even with `asHtml: false`. */
        fun cleanSynopsis(raw: String?): String? {
            if (raw.isNullOrBlank()) return null
            val text = raw
                .replace(Regex("(?i)<br\\s*/?>"), "\n")
                .replace(Regex("<[^>]+>"), "")
                .replace("&quot;", "\"").replace("&amp;", "&").replace("&#039;", "'").replace("&lt;", "<").replace("&gt;", ">")
                .replace(Regex("\n{3,}"), "\n\n")
                .trim()
            return text.ifEmpty { null }
        }
    }
}
