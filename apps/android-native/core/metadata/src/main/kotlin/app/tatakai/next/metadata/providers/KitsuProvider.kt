package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.Mappers
import app.tatakai.next.metadata.Mappers.arr
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.Mappers.str
import app.tatakai.next.metadata.Mappers.sub
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.metadata.fetchJson
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.serialization.json.JsonObject
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request
import java.time.LocalDate

/** Kitsu JSON:API (anime only). Query parameters follow legacy src/lib/metadata/providers/kitsu.ts. */
class KitsuProvider(
    private val client: OkHttpClient,
    private val baseUrl: String = "https://kitsu.io/api/edge",
    private val today: () -> LocalDate = { LocalDate.now() },
) : MetadataProvider {
    override val id = ID
    override val kinds = setOf(MediaKind.ANIME)
    override val capabilities = setOf(Capability.FEED, Capability.SEARCH, Capability.MEDIA, Capability.EPISODES)

    private fun url(path: String, params: Map<String, String?>): HttpUrl {
        val b = (baseUrl.trimEnd('/') + path).toHttpUrl().newBuilder()
        for ((k, v) in params) if (v != null) b.addQueryParameter(k, v)
        return b.build()
    }

    private suspend fun get(path: String, params: Map<String, String?>): JsonObject =
        client.fetchJson(ID, Request.Builder().url(url(path, params)).header("Accept", "application/vnd.api+json").build()).obj()
            ?: throw ProviderException(ID, 502, "unexpected payload")

    private fun listParams(page: Int): MutableMap<String, String?> = mutableMapOf(
        "include" to "mappings,genres",
        "page[limit]" to PAGE_SIZE.toString(),
        "page[offset]" to ((page.coerceAtLeast(1) - 1) * PAGE_SIZE).toString(),
        "fields[mappings]" to "externalSite,externalId",
        "fields[genres]" to "name",
    )

    override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
        val p = listParams(page)
        when (feed) {
            FeedKind.TRENDING, FeedKind.POPULAR -> p["sort"] = "-userCount"
            FeedKind.TOP_RATED -> p["sort"] = "-averageRating"
            FeedKind.AIRING -> { p["filter[status]"] = "current"; p["sort"] = "-userCount" }
            FeedKind.UPCOMING -> { p["filter[status]"] = "upcoming,unreleased"; p["sort"] = "-userCount" }
            FeedKind.SEASONAL -> {
                val now = today()
                p["filter[season]"] = when (now.monthValue) { in 1..3 -> "winter"; in 4..6 -> "spring"; in 7..9 -> "summer"; else -> "fall" }
                p["filter[seasonYear]"] = now.year.toString()
                p["sort"] = "-userCount"
            }
            // `-endDate` + a status filter 500s on Kitsu; `-startDate` is the stable proxy.
            FeedKind.RECENTLY_COMPLETED -> { p["filter[status]"] = "finished"; p["sort"] = "-startDate" }
        }
        return Mappers.kitsuPage(get("/anime", p), page)
    }

    override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage {
        val p = listParams(page)
        p["filter[text]"] = query.trim().ifEmpty { null }
        return Mappers.kitsuPage(get("/anime", p), page)
    }

    override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
        val kitsuId = resolveKitsuId(ref) ?: return null
        val doc = try {
            get("/anime/$kitsuId", mapOf("include" to "mappings,genres", "fields[mappings]" to "externalSite,externalId", "fields[genres]" to "name"))
        } catch (e: ProviderException) {
            if (e.status == 404) return null else throw e
        }
        val res = doc.sub("data") ?: return null
        return Mappers.kitsuMedia(doc, res)
    }

    override suspend fun episodes(ref: MediaRef): List<Episode> {
        val kitsuId = resolveKitsuId(ref) ?: return emptyList()
        val out = mutableListOf<Episode>()
        var offset = 0
        while (offset < MAX_EPISODES) {
            val doc = get(
                "/anime/$kitsuId/episodes",
                mapOf("page[limit]" to PAGE_SIZE.toString(), "page[offset]" to offset.toString(), "sort" to "number"),
            )
            val rows = doc.child("data").arr()?.size ?: 0
            out += Mappers.kitsuEpisodes(doc)
            if (rows < PAGE_SIZE) break
            offset += PAGE_SIZE
        }
        return out.distinctBy { it.number }.sortedBy { it.number }
    }

    private suspend fun resolveKitsuId(ref: MediaRef): Int? {
        ref.kitsuId?.let { return it }
        val (site, extId) = when {
            ref.anilistId != null -> "anilist/anime" to ref.anilistId
            ref.malId != null -> "myanimelist/anime" to ref.malId
            else -> return null
        }
        val doc = get(
            "/mappings",
            mapOf(
                "filter[externalSite]" to site,
                "filter[externalId]" to extId.toString(),
                "include" to "item",
                "fields[anime]" to "slug",
            ),
        )
        val item = doc.child("data").arr()?.firstNotNullOfOrNull { it.obj() }
            ?.sub("relationships")?.sub("item")?.sub("data")
        if (item?.str("type") != "anime") return null
        return item.str("id")?.toIntOrNull()
    }

    private fun JsonObject.child(key: String) = this[key]

    companion object {
        const val ID = "kitsu"
        private const val PAGE_SIZE = 20
        private const val MAX_EPISODES = 2000
    }
}
