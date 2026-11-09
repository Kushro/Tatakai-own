// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.metadata.Mappers.arr
import app.tatakai.next.metadata.Mappers.bool
import app.tatakai.next.metadata.Mappers.child
import app.tatakai.next.metadata.Mappers.int
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.Mappers.str
import app.tatakai.next.metadata.Mappers.sub
import app.tatakai.next.model.Character
import app.tatakai.next.model.CharacterAppearance
import app.tatakai.next.model.CharacterEdge
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.CharactersPage
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.ImageMatch
import app.tatakai.next.model.MediaDetails
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.MediaTag
import app.tatakai.next.model.NextAiring
import app.tatakai.next.model.Recommendation
import app.tatakai.next.model.StaffEdge
import app.tatakai.next.model.StaffRef
import app.tatakai.next.model.Trailer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import java.util.Locale
import kotlin.math.roundToInt

/**
 * Parity wave A (worker o): AniList "Descubrir y ficha" payloads (and trace.moe) -> core model. Pure functions over the parsed JSON so
 * the fixture tests (`DiscoverMappersTest`, contracts/fixtures/metadata/anilist-*.json) pin them down.
 */
object DiscoverMappers {

    private fun fmt(pattern: String, vararg args: Any): String = String.format(Locale.ROOT, pattern, *args)

    private fun JsonElement?.objects(): List<JsonObject> = this.arr().orEmpty().mapNotNull { it.obj() }

    private fun JsonObject.strings(key: String): List<String> =
        child(key).arr().orEmpty().mapNotNull { (it as? JsonPrimitive)?.content?.takeIf { s -> s.isNotBlank() } }

    private fun JsonObject.media(): JsonObject? = sub("data")?.sub("Media")

    /** AniList `FuzzyDate` -> `2023`, `2023-09` or `2023-09-29`; null without a year. */
    fun fuzzyDate(o: JsonObject?): String? {
        val year = o?.int("year") ?: return null
        val month = o.int("month") ?: return year.toString()
        val day = o.int("day") ?: return fmt("%04d-%02d", year, month)
        return fmt("%04d-%02d-%02d", year, month, day)
    }

    // ---------------------------------------------------------------- details

    /** [doc] = `{data:{Media:{...}}}` of the details query. */
    fun details(doc: JsonElement): MediaDetails? {
        val m = doc.obj()?.media() ?: return null
        if ((m.int("id") ?: 0) == 0) return null
        val studioEdges = m.sub("studios")?.child("edges").objects()
        // Main studios first, like the classic facts block.
        val sortedStudios = studioEdges.sortedByDescending { it.bool("isMain") }
        val studios = sortedStudios.filter { it.sub("node")?.bool("isAnimationStudio") == true }
            .mapNotNull { it.sub("node")?.str("name") }.distinct()
        val producers = sortedStudios.filter { it.sub("node")?.bool("isAnimationStudio") != true }
            .mapNotNull { it.sub("node")?.str("name") }.distinct()
        val tags = m.child("tags").objects().mapNotNull { t ->
            val name = t.str("name") ?: return@mapNotNull null
            MediaTag(name = name, rank = t.int("rank"), isSpoiler = t.bool("isMediaSpoiler") || t.bool("isGeneralSpoiler"), category = t.str("category"))
        }
        val trailer = m.sub("trailer")?.let { t ->
            val site = t.str("site")
            val id = t.str("id")
            if (site != null && id != null) Trailer(site = site, id = id, thumbnail = t.str("thumbnail")) else null
        }
        val next = m.sub("nextAiringEpisode")?.let { n ->
            val ep = n.int("episode")
            val at = (n.child("airingAt") as? JsonPrimitive)?.content?.toLongOrNull()
            if (ep != null && at != null) NextAiring(ep, at) else null
        }
        val links = linkedMapOf<String, String>()
        m.child("externalLinks").objects().forEach { l ->
            val site = l.str("site") ?: return@forEach
            val url = l.str("url") ?: return@forEach
            if (url.startsWith("https://") || url.startsWith("http://")) links.putIfAbsent(site, url)
        }
        return MediaDetails(
            ref = MediaRef(anilistId = m.int("id"), malId = m.int("idMal")),
            format = m.str("format"),
            source = m.str("source"),
            durationMin = m.int("duration"),
            startDate = fuzzyDate(m.sub("startDate")),
            endDate = fuzzyDate(m.sub("endDate")),
            meanScore = m.int("meanScore"),
            popularity = m.int("popularity"),
            favourites = m.int("favourites"),
            studios = studios,
            producers = producers,
            countryOfOrigin = m.str("countryOfOrigin"),
            tags = tags.sortedByDescending { it.rank ?: 0 },
            synonyms = m.strings("synonyms").distinct(),
            trailer = trailer,
            nextAiring = next,
            isAdult = m.bool("isAdult"),
            externalLinks = links,
        )
    }

    /** `relations.edges` of the details / relations query. Nodes without id are dropped; order = AniList order. */
    fun relations(doc: JsonElement): List<MediaRelation> =
        relationsOf(doc.obj()?.media() ?: return emptyList())

    internal fun relationsOf(m: JsonObject): List<MediaRelation> =
        m.sub("relations")?.child("edges").objects().mapNotNull { e ->
            val node = e.sub("node") ?: return@mapNotNull null
            if ((node.int("id") ?: 0) == 0) return@mapNotNull null
            MediaRelation(relation = e.str("relationType") ?: "OTHER", media = Mappers.aniListMedia(node))
        }

    fun recommendations(doc: JsonElement): List<Recommendation> {
        val m = doc.obj()?.media() ?: return emptyList()
        return m.sub("recommendations")?.child("nodes").objects().mapNotNull { r ->
            val node = r.sub("mediaRecommendation") ?: return@mapNotNull null
            if ((node.int("id") ?: 0) == 0) return@mapNotNull null
            Recommendation(media = Mappers.aniListMedia(node), rating = r.int("rating") ?: 0)
        }
    }

    // ---------------------------------------------------------------- characters / staff

    private fun staffRef(o: JsonObject, language: String?): StaffRef? {
        val id = o.int("id") ?: return null
        val name = o.sub("name")?.let { it.str("full") ?: it.str("native") } ?: return null
        val image = o.sub("image")?.let { it.str("large") ?: it.str("medium") }
        return StaffRef(id = id, name = name, image = image, language = language ?: o.str("languageV2")?.uppercase())
    }

    private fun characterRef(o: JsonObject): CharacterRef? {
        val id = o.int("id") ?: return null
        val name = o.sub("name")?.let { it.str("full") ?: it.str("native") } ?: return null
        return CharacterRef(id = id, name = name, image = o.sub("image")?.let { it.str("large") ?: it.str("medium") })
    }

    /** Aliased voice actor lists (`vaJa`, `vaEn`, ...) of one edge, de-duplicated, Japanese first. */
    private fun voiceActors(edge: JsonObject): List<StaffRef> {
        val out = linkedMapOf<Int, StaffRef>()
        for ((alias, language) in VA_ALIASES) {
            edge.child(alias).objects().forEach { va -> staffRef(va, language)?.let { out.putIfAbsent(it.id, it) } }
        }
        // Un-aliased `voiceActors` (AniList default = Japanese).
        edge.child("voiceActors").objects().forEach { va -> staffRef(va, null)?.let { out.putIfAbsent(it.id, it) } }
        return out.values.toList()
    }

    fun charactersPage(doc: JsonElement, page: Int): CharactersPage {
        val conn = doc.obj()?.media()?.sub("characters") ?: return CharactersPage(emptyList(), page, false)
        val items = conn.child("edges").objects().mapNotNull { e ->
            val c = e.sub("node")?.let(::characterRef) ?: return@mapNotNull null
            CharacterEdge(character = c, role = e.str("role"), voiceActors = voiceActors(e))
        }
        val info = conn.sub("pageInfo")
        return CharactersPage(items = items, page = info?.int("currentPage") ?: page, hasNext = info?.bool("hasNextPage") ?: false)
    }

    fun staff(doc: JsonElement): List<StaffEdge> =
        doc.obj()?.media()?.sub("staff")?.child("edges").objects().mapNotNull { e ->
            val s = e.sub("node")?.let { staffRef(it, null) }?.copy(language = null) ?: return@mapNotNull null
            StaffEdge(staff = s, role = e.str("role"))
        }

    private val SPOILER = Regex("~!.*?!~", RegexOption.DOT_MATCHES_ALL)
    private val MD_LINK = Regex("\\[([^\\]]+)]\\([^)]+\\)")
    private val MD_BOLD = Regex("__(.+?)__|\\*\\*(.+?)\\*\\*")

    /** AniList character description: spoilers removed, markdown links / bold flattened, then the usual synopsis cleaning. */
    fun cleanCharacterDescription(text: String?): String? {
        if (text.isNullOrBlank()) return null
        val flat = text.replace(SPOILER, "")
            .replace(MD_LINK) { it.groupValues[1] }
            .replace(MD_BOLD) { it.groupValues[1].ifEmpty { it.groupValues[2] } }
        return Mappers.cleanSynopsis(flat)
    }

    fun character(doc: JsonElement): Character? {
        val c = doc.obj()?.sub("data")?.sub("Character") ?: return null
        val id = c.int("id") ?: return null
        val name = c.sub("name")
        val edges = c.sub("media")?.child("edges").objects()
        val appearances = edges.mapNotNull { e ->
            val node = e.sub("node") ?: return@mapNotNull null
            if ((node.int("id") ?: 0) == 0) return@mapNotNull null
            CharacterAppearance(media = Mappers.aniListMedia(node), role = e.str("characterRole"))
        }
        val voices = linkedMapOf<Int, StaffRef>()
        edges.forEach { e -> voiceActors(e).forEach { voices.putIfAbsent(it.id, it) } }
        return Character(
            id = id,
            name = name?.str("full") ?: name?.str("native") ?: "",
            nativeName = name?.str("native"),
            image = c.sub("image")?.let { it.str("large") ?: it.str("medium") },
            description = cleanCharacterDescription(c.str("description")),
            age = c.str("age"),
            birthday = birthday(c.sub("dateOfBirth")),
            bloodType = c.str("bloodType"),
            gender = c.str("gender"),
            favourites = c.int("favourites"),
            appearances = appearances,
            voiceActors = voices.values.toList(),
        )
    }

    /** Birthday without year is the common case (`--10-31` style): `10-31`; with year `1999-10-31`. */
    internal fun birthday(o: JsonObject?): String? {
        if (o == null) return null
        val month = o.int("month") ?: return null
        val day = o.int("day")
        val year = o.int("year")
        return when {
            year != null && day != null -> fmt("%04d-%02d-%02d", year, month, day)
            day != null -> fmt("%02d-%02d", month, day)
            else -> fmt("%02d", month)
        }
    }

    fun characterRefs(doc: JsonElement): List<CharacterRef> =
        doc.obj()?.sub("data")?.sub("Page")?.child("characters").objects().mapNotNull(::characterRef)

    // ---------------------------------------------------------------- catalogues

    fun genres(doc: JsonElement): List<String> =
        doc.obj()?.sub("data")?.child("GenreCollection").arr().orEmpty()
            .mapNotNull { (it as? JsonPrimitive)?.content?.takeIf { s -> s.isNotBlank() } }

    /** `MediaTagCollection`; adult-only tags are dropped unless [adult]. */
    fun tags(doc: JsonElement, adult: Boolean = false): List<MediaTag> =
        doc.obj()?.sub("data")?.child("MediaTagCollection").objects()
            .filter { adult || !it.bool("isAdult") }
            .mapNotNull { t ->
                val name = t.str("name") ?: return@mapNotNull null
                MediaTag(name = name, rank = null, isSpoiler = t.bool("isGeneralSpoiler"), category = t.str("category"))
            }
            .sortedWith(compareBy<MediaTag>({ it.category.orEmpty() }, { it.name }))

    /** `Studio.media` (studio catalogue): anime only, 18+ dropped unless [adult]. */
    fun studioPage(doc: JsonElement, page: Int, adult: Boolean): FeedPage {
        val conn = doc.obj()?.sub("data")?.sub("Studio")?.sub("media") ?: return FeedPage(emptyList(), page, false)
        val items = conn.child("nodes").objects()
            .filter { (it.int("id") ?: 0) != 0 && it.str("type") != "MANGA" && (adult || !it.bool("isAdult")) }
            .map(Mappers::aniListMedia)
        val info = conn.sub("pageInfo")
        return FeedPage(items, info?.int("currentPage") ?: page, info?.bool("hasNextPage") ?: false)
    }

    /**
     * `Page.mediaTrends` (one row per media and day, `TRENDING_DESC`): rows are summed per media and ordered by the total, so a title
     * that trended all week ranks above a one-day spike. 18+ dropped unless [adult].
     */
    fun trendsPage(doc: JsonElement, page: Int, adult: Boolean): FeedPage {
        val p = doc.obj()?.sub("data")?.sub("Page") ?: return FeedPage(emptyList(), page, false)
        val totals = linkedMapOf<Int, Pair<JsonObject, Int>>()
        p.child("mediaTrends").objects().forEach { row ->
            val node = row.sub("media") ?: return@forEach
            val id = node.int("id") ?: row.int("mediaId") ?: return@forEach
            if (id == 0 || (!adult && node.bool("isAdult"))) return@forEach
            val score = row.int("trending") ?: 0
            val prev = totals[id]
            totals[id] = node to ((prev?.second ?: 0) + score)
        }
        val items = totals.values.sortedByDescending { it.second }.map { Mappers.aniListMedia(it.first) }
        val info = p.sub("pageInfo")
        return FeedPage(items, info?.int("currentPage") ?: page, info?.bool("hasNextPage") ?: false)
    }

    // ---------------------------------------------------------------- trace.moe

    /**
     * trace.moe `/search?anilistInfo` response. `anilist` is an object with the info flag (or a bare id without it); `episode` can be a
     * number, a string (`"1"`, `"1|2"`), an array or null. Results keep trace.moe's order (best first) with [ImageMatch.similarity] 0..1.
     */
    fun traceMoe(doc: JsonElement): List<ImageMatch> {
        val root = doc.obj() ?: return emptyList()
        return root.child("result").objects().mapNotNull { r ->
            val anilist = r.child("anilist")
            val info = anilist.obj()
            val id = info?.int("id") ?: (anilist as? JsonPrimitive)?.content?.toIntOrNull() ?: return@mapNotNull null
            val titles = info?.sub("title")
            val title = titles?.str("english") ?: titles?.str("romaji") ?: titles?.str("native") ?: r.str("filename") ?: "#$id"
            ImageMatch(
                ref = MediaRef(anilistId = id, malId = info?.int("idMal")),
                title = title,
                episode = traceEpisode(r.child("episode")),
                fromSec = (r.child("from") as? JsonPrimitive)?.content?.toDoubleOrNull() ?: 0.0,
                toSec = (r.child("to") as? JsonPrimitive)?.content?.toDoubleOrNull() ?: 0.0,
                similarity = ((r.child("similarity") as? JsonPrimitive)?.content?.toDoubleOrNull() ?: 0.0).coerceIn(0.0, 1.0),
                previewImage = r.str("image"),
                previewVideo = r.str("video"),
            )
        }
    }

    /** trace.moe error field (`"error": "Search queue is full"`); null / empty = none. */
    fun traceMoeError(doc: JsonElement): String? = doc.obj()?.str("error")

    internal fun traceEpisode(e: JsonElement?): Int? = when (e) {
        is JsonArray -> e.firstNotNullOfOrNull { traceEpisode(it) }
        is JsonPrimitive -> e.content.substringBefore('|').trim().toDoubleOrNull()?.roundToInt()
        else -> null
    }

    /** Aliases of the voice actor lists requested per language (query `VA_FIELDS`). */
    internal val VA_ALIASES = listOf(
        "vaJa" to "JAPANESE",
        "vaEn" to "ENGLISH",
        "vaEs" to "SPANISH",
        "vaPt" to "PORTUGUESE",
        "vaKo" to "KOREAN",
    )
}
