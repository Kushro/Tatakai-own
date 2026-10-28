package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.Mappers
import app.tatakai.next.metadata.Mappers.int
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.Mappers.sub
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.metadata.fetchJson
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.time.LocalDate

/** AniList GraphQL (anime + manga catalog). Query shape follows legacy src/lib/anilist/media.ts. */
class AniListProvider(
    private val client: OkHttpClient,
    private val endpoint: String = "https://graphql.anilist.co",
    private val today: () -> LocalDate = { LocalDate.now() },
) : MetadataProvider {
    override val id = ID
    override val kinds = setOf(MediaKind.ANIME, MediaKind.MANGA)
    override val capabilities = setOf(Capability.FEED, Capability.SEARCH, Capability.MEDIA)

    override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
        val vars = mutableMapOf<String, JsonElement>(
            "page" to JsonPrimitive(page.coerceAtLeast(1)),
            "perPage" to JsonPrimitive(PER_PAGE),
            "type" to JsonPrimitive(kind.name),
            "isAdult" to JsonPrimitive(false),
        )
        if (kind == MediaKind.MANGA) vars["format_not_in"] = JsonArray(listOf(JsonPrimitive("NOVEL")))
        fun sort(s: String) { vars["sort"] = JsonArray(listOf(JsonPrimitive(s))) }
        fun status(s: String) { vars["status_in"] = JsonArray(listOf(JsonPrimitive(s))) }
        when (feed) {
            FeedKind.TRENDING -> sort("TRENDING_DESC")
            FeedKind.POPULAR -> sort("POPULARITY_DESC")
            FeedKind.TOP_RATED -> sort("SCORE_DESC")
            FeedKind.AIRING -> { status("RELEASING"); sort("POPULARITY_DESC") }
            FeedKind.UPCOMING -> { status("NOT_YET_RELEASED"); sort("POPULARITY_DESC") }
            FeedKind.RECENTLY_COMPLETED -> { status("FINISHED"); sort("END_DATE_DESC") }
            FeedKind.SEASONAL -> {
                val now = today()
                if (kind == MediaKind.ANIME) {
                    vars["season"] = JsonPrimitive(
                        when (now.monthValue) { in 1..3 -> "WINTER"; in 4..6 -> "SPRING"; in 7..9 -> "SUMMER"; else -> "FALL" },
                    )
                    vars["seasonYear"] = JsonPrimitive(now.year)
                } else {
                    // Manga has no seasons: approximate with titles that started this year.
                    vars["startDate_greater"] = JsonPrimitive(now.year * 10000)
                    vars["startDate_lesser"] = JsonPrimitive((now.year + 1) * 10000)
                }
                sort("POPULARITY_DESC")
            }
        }
        return Mappers.aniListPage(post(PAGE_QUERY, JsonObject(vars)))
    }

    override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage {
        val q = query.trim()
        val vars = mutableMapOf<String, JsonElement>(
            "page" to JsonPrimitive(page.coerceAtLeast(1)),
            "perPage" to JsonPrimitive(PER_PAGE),
            "type" to JsonPrimitive(kind.name),
            "isAdult" to JsonPrimitive(false),
            "sort" to JsonArray(listOf(JsonPrimitive(if (q.isEmpty()) "POPULARITY_DESC" else "SEARCH_MATCH"))),
        )
        if (q.isNotEmpty()) vars["search"] = JsonPrimitive(q)
        if (kind == MediaKind.MANGA) vars["format_not_in"] = JsonArray(listOf(JsonPrimitive("NOVEL")))
        return Mappers.aniListPage(post(PAGE_QUERY, JsonObject(vars)))
    }

    override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
        if (ref.anilistId == null && ref.malId == null) return null
        val vars = buildJsonObject {
            ref.anilistId?.let { put("id", it) } ?: ref.malId?.let { put("idMal", it) }
            put("type", kind.name)
        }
        val doc = try {
            post(MEDIA_QUERY, vars)
        } catch (e: ProviderException) {
            if (e.status == 404) return null else throw e
        }
        val m = doc.obj()?.sub("data")?.sub("Media") ?: return null
        if ((m.int("id") ?: 0) == 0) return null
        return Mappers.aniListMedia(m)
    }

    private suspend fun post(query: String, variables: JsonObject): JsonElement {
        val body = buildJsonObject {
            put("query", query)
            put("variables", variables)
        }.toString().toRequestBody(JSON)
        val req = Request.Builder().url(endpoint).post(body).header("Accept", "application/json").build()
        val doc = client.fetchJson(ID, req)
        val errors = (doc as? JsonObject)?.get("errors") as? JsonArray
        val data = (doc as? JsonObject)?.get("data")
        if (!errors.isNullOrEmpty() && (data == null || data is JsonNull)) {
            val status = (errors.first() as? JsonObject)?.int("status") ?: 400
            throw ProviderException(ID, status, "GraphQL error")
        }
        return doc
    }

    companion object {
        const val ID = "anilist"
        private const val PER_PAGE = 20
        private val JSON = "application/json".toMediaType()

        private const val CARD = """
fragment mediaCard on Media {
  id idMal type status season seasonYear episodes
  title { romaji english native }
  coverImage { extraLarge large color } bannerImage
  averageScore isAdult genres
  description(asHtml: false)
}"""

        internal const val PAGE_QUERY = """
query (${'$'}page: Int, ${'$'}perPage: Int, ${'$'}type: MediaType, ${'$'}sort: [MediaSort], ${'$'}search: String,
  ${'$'}format_not_in: [MediaFormat], ${'$'}status_in: [MediaStatus], ${'$'}season: MediaSeason, ${'$'}seasonYear: Int,
  ${'$'}isAdult: Boolean, ${'$'}startDate_greater: FuzzyDateInt, ${'$'}startDate_lesser: FuzzyDateInt) {
  Page(page: ${'$'}page, perPage: ${'$'}perPage) {
    pageInfo { total currentPage lastPage hasNextPage perPage }
    media(type: ${'$'}type, sort: ${'$'}sort, search: ${'$'}search, format_not_in: ${'$'}format_not_in, status_in: ${'$'}status_in,
      season: ${'$'}season, seasonYear: ${'$'}seasonYear, isAdult: ${'$'}isAdult,
      startDate_greater: ${'$'}startDate_greater, startDate_lesser: ${'$'}startDate_lesser) {
      ...mediaCard
    }
  }
}
$CARD"""

        internal const val MEDIA_QUERY = """
query (${'$'}id: Int, ${'$'}idMal: Int, ${'$'}type: MediaType) {
  Media(id: ${'$'}id, idMal: ${'$'}idMal, type: ${'$'}type) { ...mediaCard }
}
$CARD"""
    }
}
