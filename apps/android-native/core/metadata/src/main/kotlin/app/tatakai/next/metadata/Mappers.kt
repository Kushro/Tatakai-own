package app.tatakai.next.metadata

import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * Provider payload -> core model. Executable spec: contracts/fixtures/metadata/reference-mapper.mjs
 * (verified against expected.json by MappersFixtureTest).
 */
object Mappers {

    // ---- JSON helpers (JS `nn`: undefined / null / '' -> null) ----

    internal fun JsonElement?.obj(): JsonObject? = this as? JsonObject
    internal fun JsonElement?.arr(): JsonArray? = this as? JsonArray
    internal fun JsonObject.child(key: String): JsonElement? = this[key]?.takeUnless { it is JsonNull }
    internal fun JsonObject.sub(key: String): JsonObject? = child(key).obj()

    internal fun JsonObject.str(key: String): String? {
        val p = child(key) as? JsonPrimitive ?: return null
        return p.content.takeIf { it.isNotEmpty() }
    }

    internal fun JsonObject.int(key: String): Int? {
        val p = child(key) as? JsonPrimitive ?: return null
        return p.content.toIntOrNull() ?: p.content.toDoubleOrNull()?.toInt()
    }

    internal fun JsonObject.bool(key: String): Boolean = (child(key) as? JsonPrimitive)?.content == "true"

    // ---- synopsis ----

    private val BR = Regex("<br\\s*/?>", RegexOption.IGNORE_CASE)
    private val TAGS = Regex("<[^>]+>")
    private val SOURCE_TAIL = Regex("\\s*\\((?:Source|Written by)[^)]*\\)\\s*$", RegexOption.IGNORE_CASE)
    private val BLANK_LINES = Regex("\\n{3,}")

    /** AniList/Kitsu synopsis: <br> -> newline, tags stripped, trailing "(Source: ...)" removed, trimmed. */
    fun cleanSynopsis(text: String?): String? {
        if (text.isNullOrEmpty()) return null
        val s = text
            .replace(BR, "\n")
            .replace(TAGS, "")
            .replace(SOURCE_TAIL, "")
            .replace(BLANK_LINES, "\n\n")
            .trim()
        return s.ifEmpty { null }
    }

    private fun seasonOfMonth(m: Int) = when {
        m <= 3 -> "WINTER"
        m <= 6 -> "SPRING"
        m <= 9 -> "SUMMER"
        else -> "FALL"
    }

    // ---- AniList ----

    fun aniListMedia(m: JsonObject): Media {
        val t = m.sub("title")
        val titles = linkedMapOf<String, String>()
        t?.str("romaji")?.let { titles["romaji"] = it }
        t?.str("english")?.let { titles["english"] = it }
        t?.str("native")?.let { titles["native"] = it }
        val cover = m.sub("coverImage")
        return Media(
            ref = MediaRef(anilistId = m.int("id"), malId = m.int("idMal"), kitsuId = null),
            kind = if (m.str("type") == "MANGA") MediaKind.MANGA else MediaKind.ANIME,
            title = t?.str("english") ?: t?.str("romaji") ?: t?.str("native") ?: "",
            titles = titles,
            cover = cover?.str("extraLarge") ?: cover?.str("large"),
            banner = m.str("bannerImage"),
            color = cover?.str("color"),
            synopsis = cleanSynopsis(m.str("description")),
            genres = m.child("genres").arr()?.mapNotNull { (it as? JsonPrimitive)?.content } ?: emptyList(),
            episodes = m.int("episodes"),
            status = m.str("status"),
            season = m.str("season"),
            year = m.int("seasonYear"),
            score = m.int("averageScore"),
            sourceProvider = "anilist",
        )
    }

    /** `doc` is the full GraphQL response (`{data:{Page:{pageInfo, media}}}`). */
    fun aniListPage(doc: JsonElement): FeedPage {
        val page = doc.obj()?.sub("data")?.sub("Page")
        val items = page?.child("media").arr().orEmpty()
            .mapNotNull { it.obj() }
            .filter { (it.int("id") ?: 0) != 0 }
            .map(::aniListMedia)
        val info = page?.sub("pageInfo")
        return FeedPage(items, info?.int("currentPage") ?: 1, info?.bool("hasNextPage") ?: false)
    }

    // ---- ani.zip ----

    private fun cleanZipTitle(v: String?): String? = v?.replace('`', '\'')?.trim()?.ifEmpty { null }

    /** Regular episodes only (numeric keys), ascending. Backticks in titles are apostrophes. */
    fun aniZipEpisodes(raw: JsonObject): List<Episode> {
        val out = mutableListOf<Episode>()
        for ((key, v) in raw.sub("episodes").orNone()) {
            if (key.isEmpty() || !key.all { it in '0'..'9' }) continue
            val e = v.obj() ?: continue
            val t = e.sub("title")
            out += Episode(
                number = key.toInt(),
                title = cleanZipTitle(t?.str("en")) ?: cleanZipTitle(t?.str("x-jat")) ?: cleanZipTitle(t?.str("ja")),
                image = e.str("image"),
                airDate = e.str("airDateUtc") ?: e.str("airDate") ?: e.str("airdate"),
                isFiller = false,
            )
        }
        return out.sortedBy { it.number }
    }

    fun aniZipIds(raw: JsonObject): MediaRef {
        val m = raw.sub("mappings")
        return MediaRef(m?.int("anilist_id"), m?.int("mal_id"), m?.int("kitsu_id"))
    }

    private fun JsonObject?.orNone(): JsonObject = this ?: JsonObject(emptyMap())

    // ---- Kitsu ----

    private val KITSU_STATUS = mapOf(
        "current" to "RELEASING",
        "finished" to "FINISHED",
        "tba" to "NOT_YET_RELEASED",
        "unreleased" to "NOT_YET_RELEASED",
        "upcoming" to "NOT_YET_RELEASED",
    )

    /** [doc] is the JSON:API document holding [res] and its `included` resources. */
    fun kitsuMedia(doc: JsonObject, res: JsonObject): Media {
        val a = res.sub("attributes").orNone()
        val included = doc.child("included").arr().orEmpty().mapNotNull { it.obj() }
        fun rel(type: String): List<JsonObject> =
            res.sub("relationships")?.sub(type)?.child("data").arr().orEmpty().mapNotNull { d ->
                val id = d.obj()?.str("id")
                included.firstOrNull { it.str("type") == type && it.str("id") == id }
            }

        val genres = rel("genres").mapNotNull { it.sub("attributes")?.str("name") }
        val mappings = rel("mappings")
        fun ext(site: String): Int? {
            val row = mappings.firstOrNull { it.sub("attributes")?.str("externalSite") == site } ?: return null
            return row.sub("attributes")?.str("externalId")?.toDoubleOrNull()?.takeIf { it.isFinite() }?.toInt()
        }

        val start = a.str("startDate")?.let { Regex("^(\\d{4})-(\\d{2})").find(it) }
        val rating = a.str("averageRating")?.toDoubleOrNull()?.let { Math.round(it).toInt() }
        val at = a.sub("titles")
        val titles = linkedMapOf<String, String>()
        at?.str("en_jp")?.let { titles["romaji"] = it }
        (at?.str("en") ?: at?.str("en_us"))?.let { titles["english"] = it }
        at?.str("ja_jp")?.let { titles["native"] = it }
        val poster = a.sub("posterImage")
        val cover = a.sub("coverImage")
        return Media(
            ref = MediaRef(ext("anilist/anime"), ext("myanimelist/anime"), res.str("id")?.toIntOrNull()),
            kind = MediaKind.ANIME,
            title = titles["english"] ?: a.str("canonicalTitle") ?: titles["romaji"] ?: "",
            titles = titles,
            cover = poster?.str("large") ?: poster?.str("original"),
            banner = cover?.str("original") ?: cover?.str("large"),
            color = null,
            synopsis = cleanSynopsis(a.str("synopsis") ?: a.str("description")),
            genres = genres,
            episodes = a.int("episodeCount"),
            status = KITSU_STATUS[a.str("status")],
            season = start?.let { seasonOfMonth(it.groupValues[2].toInt()) },
            year = start?.let { it.groupValues[1].toInt() },
            score = rating,
            sourceProvider = "kitsu",
        )
    }

    fun kitsuPage(doc: JsonObject, page: Int): FeedPage {
        val items = doc.child("data").arr().orEmpty().mapNotNull { it.obj() }.map { kitsuMedia(doc, it) }
        val hasNext = doc.sub("links")?.str("next") != null
        return FeedPage(items, page, hasNext)
    }

    fun kitsuEpisodes(doc: JsonObject): List<Episode> =
        doc.child("data").arr().orEmpty().mapNotNull { r ->
            val a = r.obj()?.sub("attributes") ?: return@mapNotNull null
            val number = a.int("number")?.takeIf { it >= 0 } ?: return@mapNotNull null
            val th = a.sub("thumbnail")
            Episode(
                number = number,
                title = a.str("canonicalTitle"),
                image = th?.str("original") ?: th?.str("large") ?: th?.str("medium"),
                airDate = a.str("airdate"),
                isFiller = false,
            )
        }.sortedBy { it.number }
}
