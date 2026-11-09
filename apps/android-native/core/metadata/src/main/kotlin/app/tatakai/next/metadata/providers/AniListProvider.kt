package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.AiringProvider
import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.ExtraMappers
import app.tatakai.next.metadata.DiscoverMappers
import app.tatakai.next.metadata.DiscoverProvider
import app.tatakai.next.metadata.TrendWindow
import app.tatakai.next.metadata.WatchOrder
import app.tatakai.next.metadata.Mappers
import app.tatakai.next.metadata.Mappers.int
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.Mappers.sub
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.metadata.fetchJson
import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.Character
import app.tatakai.next.model.PER_PAGE_AIRING
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.CharactersPage
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaDetails
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.MediaTag
import app.tatakai.next.model.PER_PAGE_CHARACTERS
import app.tatakai.next.model.Recommendation
import app.tatakai.next.model.StaffEdge
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
) : MetadataProvider, DiscoverProvider, AiringProvider {
    override val id = ID
    override val kinds = setOf(MediaKind.ANIME, MediaKind.MANGA)
    override val capabilities = setOf(Capability.FEED, Capability.SEARCH, Capability.MEDIA, Capability.DISCOVER)

    /** Wall clock in unix seconds (trend windows); injectable for tests. */
    internal var nowEpochSec: () -> Long = { System.currentTimeMillis() / 1000 }

    /** The last few details documents: details(), relations() and recommendations() of one title share one request. */
    private val detailsDocs = object : LinkedHashMap<String, Pair<Long, JsonElement>>(16, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Pair<Long, JsonElement>>?): Boolean = size > DETAILS_MEMO
    }

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

    // ---------------------------------------------------------------- parity wave A (DiscoverProvider, worker o)

    override suspend fun searchAdvanced(kind: MediaKind, query: MediaSearchQuery, page: Int): FeedPage {
        val studio = query.studio?.trim()?.takeIf { it.isNotEmpty() }
        if (studio != null) {
            val vars = buildJsonObject {
                put("search", studio)
                put("page", page.coerceAtLeast(1))
                put("perPage", PER_PAGE)
            }
            return DiscoverMappers.studioPage(post(AniListQueries.STUDIO_MEDIA, vars), page, query.adult)
        }
        val vars = AniListQueries.advancedVariables(kind, query, page, PER_PAGE, today().year)
        return Mappers.aniListPage(post(AniListQueries.ADVANCED_SEARCH, vars))
    }

    private fun refVars(ref: MediaRef, type: String? = "ANIME"): JsonObject? {
        if (ref.anilistId == null && ref.malId == null) return null
        return buildJsonObject {
            ref.anilistId?.let { put("id", it) } ?: ref.malId?.let { put("idMal", it) }
            type?.let { put("type", it) }
        }
    }

    private suspend fun detailsDoc(ref: MediaRef): JsonElement? {
        val vars = refVars(ref) ?: return null
        val key = vars.toString()
        val now = System.currentTimeMillis()
        synchronized(detailsDocs) { detailsDocs[key] }?.let { (at, doc) -> if (now - at < DETAILS_MEMO_MS) return doc }
        val doc = try {
            post(AniListQueries.DETAILS, vars)
        } catch (e: ProviderException) {
            if (e.status == 404) return null else throw e
        }
        synchronized(detailsDocs) { detailsDocs[key] = now to doc }
        return doc
    }

    override suspend fun details(ref: MediaRef): MediaDetails? = detailsDoc(ref)?.let(DiscoverMappers::details)

    override suspend fun relations(ref: MediaRef): List<MediaRelation> = detailsDoc(ref)?.let(DiscoverMappers::relations).orEmpty()

    override suspend fun recommendations(ref: MediaRef): List<Recommendation> =
        detailsDoc(ref)?.let(DiscoverMappers::recommendations).orEmpty()

    /** Anime relations of one AniList id (watch order walk), null when the title does not exist. */
    private suspend fun relationHop(id: Int): List<MediaRelation>? {
        val doc = try {
            post(AniListQueries.RELATIONS, buildJsonObject { put("id", id) })
        } catch (e: ProviderException) {
            if (e.status == 404) return null else throw e
        }
        return DiscoverMappers.relations(doc)
    }

    override suspend fun seasons(ref: MediaRef): List<Media> {
        val startId = ref.anilistId ?: details(ref)?.ref?.anilistId ?: return emptyList()
        val first = relationHop(startId) ?: return emptyList()
        val self = media(MediaKind.ANIME, MediaRef(anilistId = startId)) ?: return emptyList()
        return WatchOrder.walk(self, first) { id -> relationHop(id).orEmpty() }
    }

    override suspend fun characters(ref: MediaRef, page: Int): CharactersPage {
        val id = ref.anilistId ?: details(ref)?.ref?.anilistId ?: return CharactersPage(emptyList(), page, false)
        val vars = buildJsonObject {
            put("id", id)
            put("page", page.coerceAtLeast(1))
            put("perPage", CHARACTERS_PER_PAGE)
        }
        return DiscoverMappers.charactersPage(post(AniListQueries.CHARACTERS, vars), page)
    }

    override suspend fun staff(ref: MediaRef): List<StaffEdge> {
        val id = ref.anilistId ?: details(ref)?.ref?.anilistId ?: return emptyList()
        val vars = buildJsonObject {
            put("id", id)
            put("perPage", CHARACTERS_PER_PAGE)
        }
        return DiscoverMappers.staff(post(AniListQueries.STAFF, vars))
    }

    override suspend fun character(id: Int): Character? {
        val doc = try {
            post(AniListQueries.CHARACTER, buildJsonObject { put("id", id) })
        } catch (e: ProviderException) {
            if (e.status == 404) return null else throw e
        }
        return DiscoverMappers.character(doc)
    }

    override suspend fun searchCharacters(query: String, page: Int): List<CharacterRef> {
        val q = query.trim()
        if (q.isEmpty()) return emptyList()
        val vars = buildJsonObject {
            put("search", q)
            put("page", page.coerceAtLeast(1))
            put("perPage", PER_PAGE_CHARACTERS)
        }
        return DiscoverMappers.characterRefs(post(AniListQueries.CHARACTER_SEARCH, vars))
    }

    override suspend fun genres(kind: MediaKind): List<String> =
        DiscoverMappers.genres(post(AniListQueries.GENRES, JsonObject(emptyMap())))

    override suspend fun tags(kind: MediaKind): List<MediaTag> =
        DiscoverMappers.tags(post(AniListQueries.TAGS, JsonObject(emptyMap())))

    override suspend fun trending(kind: MediaKind, window: TrendWindow, page: Int, adult: Boolean): FeedPage = when (window) {
        TrendWindow.TODAY -> searchAdvanced(kind, MediaSearchQuery(sort = MediaSort.TRENDING, adult = adult), page)
        TrendWindow.ALL_TIME -> searchAdvanced(kind, MediaSearchQuery(sort = MediaSort.POPULARITY, adult = adult), page)
        TrendWindow.WEEK, TrendWindow.MONTH -> {
            val vars = buildJsonObject {
                put("page", page.coerceAtLeast(1))
                put("perPage", TRENDS_PER_PAGE)
                put("date_greater", nowEpochSec() - window.days * 86_400L)
            }
            DiscoverMappers.trendsPage(post(AniListQueries.TRENDS, vars), page, adult)
        }
    }

    // ---------------------------------------------------------------- parity wave B (AiringProvider, worker r)

    override suspend fun airingSchedule(fromEpochSec: Long, toEpochSec: Long, page: Int): Pair<List<AiringEpisode>, Boolean> {
        if (toEpochSec <= fromEpochSec) return emptyList<AiringEpisode>() to false
        val vars = buildJsonObject {
            put("page", page.coerceAtLeast(1))
            put("perPage", PER_PAGE_AIRING)
            // airingAt_greater / _lesser are exclusive: `[from, to)` = `> from - 1` and `< to`.
            put("from", (fromEpochSec - 1).coerceIn(0, Int.MAX_VALUE.toLong()).toInt())
            put("to", toEpochSec.coerceIn(0, Int.MAX_VALUE.toLong()).toInt())
        }
        val doc = post(AIRING_QUERY, vars)
        return ExtraMappers.airingSchedules(doc).filter { it.airingAtEpochSec in fromEpochSec until toEpochSec } to ExtraMappers.airingHasNext(doc)
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
        private const val CHARACTERS_PER_PAGE = 25
        private const val TRENDS_PER_PAGE = 50
        private const val DETAILS_MEMO = 8
        private const val DETAILS_MEMO_MS = 5L * 60_000
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

        internal const val AIRING_QUERY = """
query (${'$'}page: Int, ${'$'}perPage: Int, ${'$'}from: Int, ${'$'}to: Int) {
  Page(page: ${'$'}page, perPage: ${'$'}perPage) {
    pageInfo { currentPage hasNextPage }
    airingSchedules(airingAt_greater: ${'$'}from, airingAt_lesser: ${'$'}to, sort: [TIME]) {
      episode airingAt
      media { ...mediaCard }
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
