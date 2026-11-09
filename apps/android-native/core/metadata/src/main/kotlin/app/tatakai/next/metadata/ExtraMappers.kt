// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.metadata.Mappers.arr
import app.tatakai.next.metadata.Mappers.child
import app.tatakai.next.metadata.Mappers.int
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.Mappers.str
import app.tatakai.next.metadata.Mappers.sub
import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlin.math.roundToInt

/**
 * Parity wave B (16 §3.1, worker r): payload -> core model for the extra metadata providers of the router (MAL v2, Shikimori GraphQL,
 * Tenrai = Jikan v4) and the AniList airing schedule. Ports of the classic `src/lib/metadata/providers/{mal-schema,shikimori,tenrai}.ts`
 * mappings (only the fields the Next `Media` carries). Scores are normalised to 0..100 like AniList's `averageScore`.
 */
object ExtraMappers {
    private fun score10(o: JsonObject, key: String): Int? {
        val v = (o.child(key) as? JsonPrimitive)?.content?.toDoubleOrNull() ?: return null
        return if (v > 0) (v * 10).roundToInt() else null
    }

    private fun seasonOfMonth(m: Int): String = when {
        m <= 3 -> "WINTER"
        m <= 6 -> "SPRING"
        m <= 9 -> "SUMMER"
        else -> "FALL"
    }

    private fun yearMonth(date: String?): Pair<Int, Int?>? {
        val m = Regex("^(\\d{4})(?:-(\\d{2}))?").find(date ?: return null) ?: return null
        return m.groupValues[1].toInt() to m.groupValues[2].takeIf { it.isNotEmpty() }?.toInt()
    }

    private fun titleOf(titles: Map<String, String>): String = titles["english"] ?: titles["romaji"] ?: titles["native"] ?: ""

    // ---- MyAnimeList API v2 ----

    private val MAL_STATUS = mapOf(
        "finished_airing" to "FINISHED", "currently_airing" to "RELEASING", "not_yet_aired" to "NOT_YET_RELEASED",
        "finished" to "FINISHED", "currently_publishing" to "RELEASING", "not_yet_published" to "NOT_YET_RELEASED",
        "on_hiatus" to "HIATUS", "discontinued" to "CANCELLED",
    )

    /** One MAL v2 node (`/anime/{id}` or `data[].node` of a list). */
    fun malNode(o: JsonObject, kind: MediaKind): Media? {
        val id = o.int("id") ?: return null
        val titles = linkedMapOf<String, String>()
        o.str("title")?.let { titles["romaji"] = it }
        val alt = o.sub("alternative_titles")
        alt?.str("en")?.let { titles["english"] = it }
        alt?.str("ja")?.let { titles["native"] = it }
        val pic = o.sub("main_picture")
        val season = o.sub("start_season")
        val start = yearMonth(o.str("start_date"))
        return Media(
            ref = MediaRef(malId = id),
            kind = kind,
            title = titleOf(titles),
            titles = titles,
            cover = pic?.str("large") ?: pic?.str("medium"),
            banner = null,
            color = null,
            synopsis = Mappers.cleanSynopsis(o.str("synopsis")),
            genres = o.child("genres").arr().orEmpty().mapNotNull { it.obj()?.str("name") },
            episodes = o.int("num_episodes")?.takeIf { it > 0 } ?: o.int("num_chapters")?.takeIf { it > 0 },
            status = MAL_STATUS[o.str("status")],
            season = season?.str("season")?.uppercase() ?: start?.second?.let(::seasonOfMonth),
            year = season?.int("year") ?: start?.first,
            score = score10(o, "mean"),
            sourceProvider = "mal",
        )
    }

    fun malPage(doc: JsonElement, kind: MediaKind, page: Int): FeedPage {
        val o = doc.obj()
        val items = o?.child("data").arr().orEmpty().mapNotNull { it.obj()?.sub("node")?.let { n -> malNode(n, kind) } }
        val hasNext = o?.sub("paging")?.str("next") != null
        return FeedPage(items, page, hasNext)
    }

    // ---- Shikimori GraphQL ----

    private val SHIKI_STATUS = mapOf("anons" to "NOT_YET_RELEASED", "ongoing" to "RELEASING", "released" to "FINISHED", "paused" to "HIATUS", "discontinued" to "CANCELLED")
    private val CYRILLIC = Regex("[\\u0400-\\u04FF]")
    private val BBCODE = Regex("\\[/?[a-z_]+(?:=[^\\]]*)?]", RegexOption.IGNORE_CASE)
    private val SHIKI_SEASON = Regex("^(winter|spring|summer|fall)_(\\d{4})$")

    fun shikiEntry(o: JsonObject, kind: MediaKind): Media? {
        val malId = o.str("malId")?.toIntOrNull() ?: o.str("id")?.toIntOrNull() ?: return null
        val titles = linkedMapOf<String, String>()
        o.str("name")?.let { titles["romaji"] = it }
        o.str("english")?.let { titles["english"] = it }
        o.str("japanese")?.let { titles["native"] = it }
        val poster = o.sub("poster")
        val description = o.str("description")?.takeIf { !CYRILLIC.containsMatchIn(it) }?.replace(BBCODE, "")
        val seasonMatch = o.str("season")?.let { SHIKI_SEASON.find(it) }
        val aired = o.sub("airedOn")
        return Media(
            ref = MediaRef(malId = malId),
            kind = kind,
            title = titleOf(titles),
            titles = titles,
            cover = poster?.str("originalUrl") ?: poster?.str("mainUrl"),
            banner = null,
            color = null,
            synopsis = Mappers.cleanSynopsis(description),
            genres = o.child("genres").arr().orEmpty().mapNotNull { it.obj()?.str("name") }.distinct(),
            episodes = (o.int("episodes") ?: o.int("chapters"))?.takeIf { it > 0 },
            status = SHIKI_STATUS[o.str("status")],
            season = seasonMatch?.groupValues?.get(1)?.uppercase() ?: aired?.int("month")?.let(::seasonOfMonth),
            year = seasonMatch?.groupValues?.get(2)?.toInt() ?: aired?.int("year"),
            score = score10(o, "score"),
            sourceProvider = "shikimori",
        )
    }

    /** `{data:{animes:[...]}}` / `{data:{mangas:[...]}}`. */
    fun shikiList(doc: JsonElement, kind: MediaKind): List<Media> {
        val field = if (kind == MediaKind.MANGA) "mangas" else "animes"
        return doc.obj()?.sub("data")?.child(field).arr().orEmpty().mapNotNull { it.obj()?.let { e -> shikiEntry(e, kind) } }
    }

    // ---- Tenrai (Jikan v4 schema) ----

    private val JIKAN_STATUS = mapOf(
        "Finished Airing" to "FINISHED", "Currently Airing" to "RELEASING", "Not yet aired" to "NOT_YET_RELEASED",
        "Finished" to "FINISHED", "Publishing" to "RELEASING", "On Hiatus" to "HIATUS", "Discontinued" to "CANCELLED",
        "Not yet published" to "NOT_YET_RELEASED",
    )

    fun jikanEntry(o: JsonObject, kind: MediaKind): Media? {
        val id = o.int("mal_id") ?: return null
        val titles = linkedMapOf<String, String>()
        o.str("title")?.let { titles["romaji"] = it }
        o.str("title_english")?.let { titles["english"] = it }
        o.str("title_japanese")?.let { titles["native"] = it }
        val images = o.sub("images")
        val img = images?.sub("webp") ?: images?.sub("jpg")
        val from = o.sub("aired")?.str("from") ?: o.sub("published")?.str("from")
        val start = yearMonth(from)
        return Media(
            ref = MediaRef(malId = id),
            kind = kind,
            title = titleOf(titles),
            titles = titles,
            cover = img?.str("large_image_url") ?: img?.str("image_url"),
            banner = null,
            color = null,
            synopsis = Mappers.cleanSynopsis(o.str("synopsis")?.replace("[Written by MAL Rewrite]", "")),
            genres = (o.child("genres").arr().orEmpty() + o.child("themes").arr().orEmpty()).mapNotNull { it.obj()?.str("name") }.distinct(),
            episodes = (o.int("episodes") ?: o.int("chapters"))?.takeIf { it > 0 },
            status = JIKAN_STATUS[o.str("status")],
            season = o.str("season")?.uppercase() ?: start?.second?.let(::seasonOfMonth),
            year = o.int("year") ?: start?.first,
            score = score10(o, "score"),
            sourceProvider = "tenrai",
        )
    }

    fun jikanPage(doc: JsonElement, kind: MediaKind, page: Int): FeedPage {
        val o = doc.obj()
        val items = o?.child("data").arr().orEmpty().mapNotNull { it.obj()?.let { e -> jikanEntry(e, kind) } }.distinctBy { it.ref.malId }
        val hasNext = (o?.sub("pagination")?.child("has_next_page") as? JsonPrimitive)?.content == "true"
        return FeedPage(items, page, hasNext)
    }

    /** `/anime/{id}/episodes?page=`: returns the episodes and whether another page exists. */
    fun jikanEpisodes(doc: JsonElement): Pair<List<Episode>, Boolean> {
        val o = doc.obj()
        val list = o?.child("data").arr().orEmpty().mapNotNull { el ->
            val e = el.obj() ?: return@mapNotNull null
            val n = e.int("mal_id") ?: e.int("episode") ?: return@mapNotNull null
            Episode(
                number = n,
                title = e.str("title"),
                image = e.sub("images")?.sub("jpg")?.str("image_url"),
                airDate = e.str("aired"),
                isFiller = (e.child("filler") as? JsonPrimitive)?.content == "true",
            )
        }
        val hasNext = (o?.sub("pagination")?.child("has_next_page") as? JsonPrimitive)?.content == "true"
        return list to hasNext
    }

    // ---- AniList airing schedule ----

    /** `{data:{Page:{airingSchedules:[{episode, airingAt, media{...mediaCard}}]}}}`; adult titles are dropped. Soonest first. */
    fun airingSchedules(doc: JsonElement): List<AiringEpisode> {
        val rows = doc.obj()?.sub("data")?.sub("Page")?.child("airingSchedules").arr().orEmpty()
        return rows.mapNotNull { el ->
            val r = el.obj() ?: return@mapNotNull null
            val m = r.sub("media") ?: return@mapNotNull null
            if ((m.child("isAdult") as? JsonPrimitive)?.content == "true") return@mapNotNull null
            if ((m.int("id") ?: 0) == 0) return@mapNotNull null
            val ep = r.int("episode") ?: return@mapNotNull null
            val at = (r.child("airingAt") as? JsonPrimitive)?.content?.toLongOrNull() ?: return@mapNotNull null
            AiringEpisode(media = Mappers.aniListMedia(m), episode = ep, airingAtEpochSec = at)
        }.sortedBy { it.airingAtEpochSec }
    }

    fun airingHasNext(doc: JsonElement): Boolean =
        (doc.obj()?.sub("data")?.sub("Page")?.sub("pageInfo")?.child("hasNextPage") as? JsonPrimitive)?.content == "true"
}
