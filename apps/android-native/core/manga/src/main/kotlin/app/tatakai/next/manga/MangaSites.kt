// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.MangaChapter
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException
import java.time.OffsetDateTime

/** MangaUpdates series facts (checklist manga.metadata.mangaupdates): publishers, serialization, publication status. */
data class MangaUpdatesSeries(
    val id: Long,
    val title: String,
    /** Free text of MangaUpdates (`"41 Volumes (Ongoing)"`). */
    val status: String? = null,
    val completed: Boolean = false,
    val licensed: Boolean = false,
    /** Original publishers first, then English / other. */
    val publishers: List<String> = emptyList(),
    /** Magazine(s) of the serialization (`publications`). */
    val serialization: String? = null,
    /** 0..100 (bayesian rating x 10). */
    val rating: Int? = null,
    val year: Int? = null,
    val latestChapter: Int? = null,
    val url: String? = null,
)

internal suspend fun OkHttpClient.getJson(request: Request, site: String): JsonElement = withContext(Dispatchers.IO) {
    val text = try {
        newCall(request).execute().use { response ->
            if (response.code == 429) throw AppException(AppError.RateLimited(site))
            if (!response.isSuccessful) throw AppException(AppError.Http(response.code))
            response.body.string()
        }
    } catch (e: IOException) {
        throw AppException(AppError.Offline, e)
    }
    runCatching { ReaderJson.parseToJsonElement(text) }.getOrElse { throw AppException(AppError.Unknown("$site: bad JSON"), it) }
}

/** `api.mangaupdates.com/v1` (public, no key): series search + series page. */
class MangaUpdatesClient(
    private val client: OkHttpClient,
    private val baseUrl: String = BASE_URL,
) {
    suspend fun search(title: String): List<Pair<Long, String>> {
        val body = buildJsonObject {
            put("search", title)
            put("perpage", 8)
        }.toString().toRequestBody(JSON)
        val doc = client.getJson(Request.Builder().url("$baseUrl/series/search").post(body).build(), "MangaUpdates")
        return parseSearch(doc)
    }

    suspend fun series(id: Long): MangaUpdatesSeries? {
        val doc = client.getJson(Request.Builder().url("$baseUrl/series/$id").get().build(), "MangaUpdates")
        return parseSeries(doc.obj() ?: return null)
    }

    /** Search by every known title and take the best match (year within one when both are known). */
    suspend fun find(titles: List<String>, year: Int?): MangaUpdatesSeries? {
        for (title in titles.filter { it.isNotBlank() }.distinct().take(3)) {
            val hits = search(title)
            val best = hits
                .map { (id, name) -> Triple(id, name, TitleMatch.similarity(name, title)) }
                .filter { it.third >= TitleMatch.MATCH_THRESHOLD }
                .maxByOrNull { it.third } ?: continue
            val series = series(best.first) ?: continue
            if (year != null && series.year != null && kotlin.math.abs(series.year - year) > 1) continue
            return series
        }
        return null
    }

    companion object {
        const val BASE_URL: String = "https://api.mangaupdates.com/v1"
        private val JSON = "application/json; charset=utf-8".toMediaType()

        fun parseSearch(doc: JsonElement): List<Pair<Long, String>> = doc.obj().list("results").mapNotNull { r ->
            val record = r.sub("record") ?: return@mapNotNull null
            val id = record.long("series_id") ?: return@mapNotNull null
            val title = r.str("hit_title") ?: record.str("title") ?: return@mapNotNull null
            id to title
        }

        fun parseSeries(o: JsonObject): MangaUpdatesSeries? {
            val id = o.long("series_id") ?: return null
            val publishers = o.list("publishers")
                .sortedBy { if (it.str("type").equals("Original", ignoreCase = true)) 0 else 1 }
                .mapNotNull { it.str("publisher_name") }
                .distinct()
            val serialization = o.list("publications").mapNotNull { it.str("publication_name") }.distinct().joinToString(", ").ifBlank { null }
            return MangaUpdatesSeries(
                id = id,
                title = o.str("title") ?: "",
                status = o.str("status")?.replace(Regex("\\s+"), " ")?.trim(),
                completed = o.bool("completed"),
                licensed = o.bool("licensed"),
                publishers = publishers,
                serialization = serialization,
                rating = o.double("bayesian_rating")?.let { (it * 10).toInt().coerceIn(0, 100) },
                year = o.str("year")?.take(4)?.toIntOrNull(),
                latestChapter = o.int("latest_chapter"),
                url = o.str("url"),
            )
        }
    }
}

/**
 * `api.mangadex.org` (public): chapter-list fallback when no manga extension is installed (16 §7 #6), exposed as a synthetic source
 * `extensionId = "mangadex"`. Titles are matched by the AniList link of MangaDex (`links.al`) first, then by title.
 */
class MangaDexClient(
    private val client: OkHttpClient,
    private val baseUrl: String = BASE_URL,
) {
    /** MangaDex id of a title: AniList link match first, then the best title match. */
    suspend fun find(anilistId: Int?, titles: List<String>): String? {
        val title = titles.firstOrNull { it.isNotBlank() } ?: return null
        val url = "$baseUrl/manga".toHttpUrl().newBuilder()
            .addQueryParameter("title", title)
            .addQueryParameter("limit", "10")
            .apply { CONTENT_RATINGS.forEach { addQueryParameter("contentRating[]", it) } }
            .build()
        val doc = client.getJson(Request.Builder().url(url).get().build(), "MangaDex")
        return pickManga(doc, anilistId, titles)
    }

    /** Every chapter of [mangaId] in [languages] (paged, up to [MAX_CHAPTERS]), mapped to [MangaChapter]s of the synthetic source. */
    suspend fun chapters(mangaId: String, languages: List<String>): List<MangaChapter> {
        val out = ArrayList<MangaChapter>()
        var offset = 0
        while (offset < MAX_CHAPTERS) {
            val url = "$baseUrl/manga/$mangaId/feed".toHttpUrl().newBuilder()
                .addQueryParameter("limit", PAGE.toString())
                .addQueryParameter("offset", offset.toString())
                .addQueryParameter("order[volume]", "asc")
                .addQueryParameter("order[chapter]", "asc")
                .addQueryParameter("includes[]", "scanlation_group")
                .apply {
                    languages.forEach { addQueryParameter("translatedLanguage[]", it) }
                    CONTENT_RATINGS.forEach { addQueryParameter("contentRating[]", it) }
                }
                .build()
            val doc = client.getJson(Request.Builder().url(url).get().build(), "MangaDex")
            val page = parseFeed(doc, mangaId)
            out += page
            val total = doc.obj().int("total") ?: 0
            offset += PAGE
            if (offset >= total || doc.obj().list("data").isEmpty()) break
        }
        return out
    }

    suspend fun pages(chapterId: String): List<ChapterPage> {
        val doc = client.getJson(Request.Builder().url("$baseUrl/at-home/server/$chapterId").get().build(), "MangaDex")
        return parseAtHome(doc)
    }

    companion object {
        const val BASE_URL: String = "https://api.mangadex.org"
        const val EXTENSION_ID: String = "mangadex"
        const val SOURCE_ID: String = "mangadex"
        const val NAME: String = "MangaDex"
        private const val PAGE = 500
        private const val MAX_CHAPTERS = 3000
        private val CONTENT_RATINGS = listOf("safe", "suggestive", "erotica")

        fun pickManga(doc: JsonElement, anilistId: Int?, titles: List<String>): String? {
            val data = doc.obj().list("data")
            if (anilistId != null) {
                data.firstOrNull { it.sub("attributes").sub("links").str("al") == anilistId.toString() }?.str("id")?.let { return it }
            }
            return data.map { m ->
                val attrs = m.sub("attributes")
                val names = attrs.sub("title")?.values?.mapNotNull { (it as? kotlinx.serialization.json.JsonPrimitive)?.content }.orEmpty() +
                    attrs.list("altTitles").flatMap { alt -> alt.values.mapNotNull { (it as? kotlinx.serialization.json.JsonPrimitive)?.content } }
                val score = names.maxOfOrNull { n -> titles.maxOfOrNull { TitleMatch.similarity(n, it) } ?: 0.0 } ?: 0.0
                m.str("id") to score
            }.filter { it.first != null && it.second >= TitleMatch.MATCH_THRESHOLD }.maxByOrNull { it.second }?.first
        }

        fun parseFeed(doc: JsonElement, mangaId: String): List<MangaChapter> = doc.obj().list("data").mapNotNull { c ->
            val id = c.str("id") ?: return@mapNotNull null
            val a = c.sub("attributes")
            if (a.str("externalUrl") != null && (a.int("pages") ?: 0) == 0) return@mapNotNull null
            val group = c.list("relationships").firstOrNull { it.str("type") == "scanlation_group" }.sub("attributes").str("name")
            MangaChapter(
                id = id,
                extensionId = EXTENSION_ID,
                sourceId = SOURCE_ID,
                itemId = mangaId,
                number = a.str("chapter")?.toDoubleOrNull(),
                volume = a.str("volume")?.toDoubleOrNull()?.toInt(),
                title = a.str("title"),
                lang = a.str("translatedLanguage")?.substringBefore('-'),
                group = group,
                releasedAtMs = a.str("publishAt")?.let { runCatching { OffsetDateTime.parse(it).toInstant().toEpochMilli() }.getOrNull() },
            )
        }

        fun parseAtHome(doc: JsonElement): List<ChapterPage> {
            val o = doc.obj()
            val base = o.str("baseUrl") ?: return emptyList()
            val chapter = o.sub("chapter")
            val hash = chapter.str("hash") ?: return emptyList()
            return chapter.strings("data").mapIndexed { i, file -> ChapterPage(i, "$base/data/$hash/$file") }
        }
    }
}
