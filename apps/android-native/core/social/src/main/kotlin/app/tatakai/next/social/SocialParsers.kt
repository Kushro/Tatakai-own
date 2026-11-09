// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import app.tatakai.next.model.ActivityReply
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.Review
import app.tatakai.next.model.SocialActivity
import app.tatakai.next.model.SocialNotification
import app.tatakai.next.model.SocialPage
import app.tatakai.next.model.SocialStats
import app.tatakai.next.model.SocialUser
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.longOrNull

// Parity wave B (16 §3.7, worker t): AniList GraphQL answers -> the social model. Pure JVM (unit-tested with recorded fixtures); a field
// that does not fit is skipped, a node without its id is dropped.

object SocialParsers {
    /** `User` (+ optional follow counts from the `Page.pageInfo.total` aliases). */
    fun user(o: JsonObject?, viewerId: Int?, followers: Int = 0, following: Int = 0): SocialUser? {
        o ?: return null
        val id = o.int("id") ?: return null
        val name = o.str("name") ?: return null
        val stats = o.obj("statistics")?.let { s ->
            val anime = s.obj("anime")
            val manga = s.obj("manga")
            SocialStats(
                animeCount = anime?.int("count") ?: 0,
                episodesWatched = anime?.int("episodesWatched") ?: 0,
                minutesWatched = anime?.long("minutesWatched") ?: 0L,
                meanScore = anime?.double("meanScore")?.takeIf { it > 0 }?.toFloat(),
                mangaCount = manga?.int("count") ?: 0,
                chaptersRead = manga?.int("chaptersRead") ?: 0,
            )
        }
        val fav = o.obj("favourites")
        return SocialUser(
            id = id,
            name = name,
            avatar = o.obj("avatar")?.let { it.str("large") ?: it.str("medium") },
            banner = o.str("bannerImage"),
            about = o.str("about")?.let(::plainText)?.takeIf { it.isNotBlank() },
            followers = followers,
            following = following,
            isFollowing = o.bool("isFollowing") ?: false,
            isFollower = o.bool("isFollower") ?: false,
            isViewer = viewerId != null && viewerId == id,
            siteUrl = o.str("siteUrl") ?: "https://anilist.co/user/$name",
            stats = stats,
            favoriteAnime = nodes(fav?.obj("anime")).mapNotNull { media(it) },
            favoriteManga = nodes(fav?.obj("manga")).mapNotNull { media(it) },
            favoriteCharacters = nodes(fav?.obj("characters")).mapNotNull { c ->
                val cid = c.int("id") ?: return@mapNotNull null
                CharacterRef(cid, c.obj("name")?.let { it.str("userPreferred") ?: it.str("full") } ?: return@mapNotNull null, c.obj("image")?.let { it.str("large") ?: it.str("medium") })
            },
        )
    }

    /** `Media` node -> [Media] (the fields the social screens show; the rest stays empty). */
    fun media(o: JsonObject?): Media? {
        o ?: return null
        val id = o.int("id") ?: return null
        val t = o.obj("title")
        val title = t?.let { it.str("userPreferred") ?: it.str("romaji") ?: it.str("english") ?: it.str("native") } ?: return null
        val titles = buildMap {
            t?.str("romaji")?.let { put("romaji", it) }
            t?.str("english")?.let { put("english", it) }
            t?.str("native")?.let { put("native", it) }
        }
        return Media(
            ref = MediaRef(anilistId = id, malId = o.int("idMal")),
            kind = if (o.str("type") == "MANGA") MediaKind.MANGA else MediaKind.ANIME,
            title = title,
            titles = titles,
            cover = o.obj("coverImage")?.let { it.str("large") ?: it.str("medium") },
            banner = o.str("bannerImage"),
            color = o.obj("coverImage")?.str("color"),
            synopsis = null,
            genres = (o["genres"] as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.contentOrNull },
            episodes = o.int("episodes"),
            status = o.str("status"),
            season = o.str("season"),
            year = o.int("seasonYear"),
            score = o.int("averageScore"),
            sourceProvider = "anilist",
        )
    }

    /** `ActivityUnion` node. Unknown / empty fragments (an activity type this query does not select) -> null. */
    fun activity(o: JsonObject?, viewerId: Int?): SocialActivity? {
        o ?: return null
        val id = o.int("id") ?: return null
        val createdAt = o.long("createdAt") ?: 0L
        val likes = o.int("likeCount") ?: 0
        val liked = o.bool("isLiked") ?: false
        val replies = o.int("replyCount") ?: 0
        val site = o.str("siteUrl") ?: "https://anilist.co/activity/$id"
        val media = o.obj("media")
        return when {
            media != null -> {
                val user = user(o.obj("user"), viewerId) ?: return null
                SocialActivity.ListActivity(
                    id = id, user = user, createdAtSec = createdAt, likes = likes, liked = liked, replyCount = replies, siteUrl = site,
                    media = media(media) ?: return null,
                    status = o.str("status").orEmpty(),
                    progress = o.str("progress") ?: o.int("progress")?.toString(),
                )
            }
            o.containsKey("message") -> {
                val sender = user(o.obj("messenger"), viewerId) ?: return null
                SocialActivity.TextActivity(
                    id = id, user = sender, createdAtSec = createdAt, likes = likes, liked = liked, replyCount = replies, siteUrl = site,
                    text = plainText(o.str("message").orEmpty()),
                    recipient = user(o.obj("recipient"), viewerId),
                )
            }
            o.containsKey("text") -> {
                val user = user(o.obj("user"), viewerId) ?: return null
                SocialActivity.TextActivity(
                    id = id, user = user, createdAtSec = createdAt, likes = likes, liked = liked, replyCount = replies, siteUrl = site,
                    text = plainText(o.str("text").orEmpty()),
                )
            }
            else -> null
        }
    }

    fun reply(o: JsonObject?, viewerId: Int?): ActivityReply? {
        o ?: return null
        val id = o.int("id") ?: return null
        val user = user(o.obj("user"), viewerId) ?: return null
        return ActivityReply(id, user, plainText(o.str("text").orEmpty()), o.long("createdAt") ?: 0L, o.int("likeCount") ?: 0, o.bool("isLiked") ?: false)
    }

    /**
     * `NotificationUnion` node. [unread] comes from the caller (AniList marks the first `unreadNotificationCount` rows as new); [texts]
     * builds the localised sentence from the type, falling back to AniList's own `context` / `contexts`.
     */
    fun notification(o: JsonObject?, viewerId: Int?, texts: SocialTexts, unread: Boolean): SocialNotification? {
        o ?: return null
        val id = o.int("id") ?: return null
        val type = o.str("type") ?: return null
        val user = user(o.obj("user"), viewerId)
        val media = media(o.obj("media"))
        val episode = o.int("episode")
        val contexts = (o["contexts"] as? JsonArray).orEmpty().map { (it as? JsonPrimitive)?.contentOrNull.orEmpty() }
        val context = o.str("context")
        val fallback = contextText(type, contexts, context, user?.name, media?.title, episode, o.str("deletedMediaTitle"))
        return SocialNotification(
            id = id,
            type = type,
            createdAtSec = o.long("createdAt") ?: 0L,
            text = texts.notification(type, user?.name, media?.title ?: o.str("deletedMediaTitle"), episode, fallback),
            media = media,
            user = user,
            episode = episode,
            activityId = o.int("activityId"),
            unread = unread,
        )
    }

    /**
     * AniList's own sentence: `AIRING` joins `contexts` around the episode and the title (`["Episode ", " of ", " aired."]`); the
     * others put the user's name before `context` (`" started following you."`) or the title before it (media notifications).
     */
    fun contextText(type: String, contexts: List<String>, context: String?, userName: String?, mediaTitle: String?, episode: Int?, deletedTitle: String?): String {
        if (type == "AIRING" && contexts.isNotEmpty()) {
            val parts = listOf(episode?.toString().orEmpty(), mediaTitle.orEmpty())
            return buildString {
                contexts.forEachIndexed { i, c ->
                    append(c)
                    if (i < parts.size && i < contexts.size - 1) append(parts[i])
                }
            }.trim()
        }
        val c = context ?: contexts.joinToString("")
        val subject = userName ?: mediaTitle ?: deletedTitle ?: ""
        return (subject + c).trim()
    }

    fun review(o: JsonObject?, viewerId: Int?): Review? {
        o ?: return null
        val id = o.int("id") ?: return null
        val user = user(o.obj("user"), viewerId) ?: return null
        return Review(
            id = id,
            user = user,
            score = o.int("score") ?: 0,
            summary = o.str("summary").orEmpty(),
            body = plainText(o.str("body").orEmpty()),
            rating = o.int("rating") ?: 0,
            ratingAmount = o.int("ratingAmount") ?: 0,
            createdAtSec = o.long("createdAt") ?: 0L,
            siteUrl = o.str("siteUrl") ?: "https://anilist.co/review/$id",
        )
    }

    /** `Page { pageInfo { currentPage hasNextPage } <key> [...] }` -> a [SocialPage] of the parsed nodes. */
    fun <T> page(data: JsonObject?, key: String, page: Int, parse: (JsonObject) -> T?): SocialPage<T> {
        val p = data?.obj("Page")
        val items = (p?.get(key) as? JsonArray).orEmpty().mapNotNull { (it as? JsonObject)?.let(parse) }
        val info = p?.obj("pageInfo")
        return SocialPage(items, info?.int("currentPage") ?: page, info?.bool("hasNextPage") ?: false)
    }

    /**
     * AniList markdown / HTML-ish text -> plain text: `img###(url)` / `~~~` / `__` / `**` markers, `<br>`, tags and entities removed;
     * links keep their label. Good enough for the about card and activity bodies (AniList renders them as markdown).
     */
    fun plainText(raw: String): String {
        var s = raw.replace(Regex("(?i)<br\\s*/?>"), "\n")
        s = s.replace(Regex("(?i)img\\d*%?\\((.*?)\\)"), "")
        s = s.replace(Regex("(?i)(youtube|webm)\\((.*?)\\)"), "$2")
        s = s.replace(Regex("\\[([^\\]]*)]\\(([^)]*)\\)"), "$1")
        s = s.replace(Regex("<[^>]+>"), "")
        s = s.replace("~~~", "").replace("~!", "").replace("!~", "").replace("__", "").replace("**", "")
        s = s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#039;", "'").replace("&nbsp;", " ")
        return s.lines().joinToString("\n") { it.trimEnd() }.replace(Regex("\n{3,}"), "\n\n").trim()
    }

    private fun nodes(o: JsonObject?): List<JsonObject> = (o?.get("nodes") as? JsonArray).orEmpty().mapNotNull { it as? JsonObject }
}

internal fun JsonObject.obj(key: String): JsonObject? = this[key] as? JsonObject

internal fun JsonObject.str(key: String): String? =
    (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull && it.isString }?.content?.takeIf { it.isNotBlank() }

internal fun JsonObject.int(key: String): Int? = (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.intOrNull

internal fun JsonObject.long(key: String): Long? = (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.longOrNull

internal fun JsonObject.double(key: String): Double? = (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull }?.doubleOrNull

internal fun JsonObject.bool(key: String): Boolean? = (this[key] as? JsonPrimitive)?.takeIf { it !is JsonNull && !it.isString }?.booleanOrNull
