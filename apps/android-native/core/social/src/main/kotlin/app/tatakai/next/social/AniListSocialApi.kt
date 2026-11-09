// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException

// Parity wave B (16 §3.7, worker t): the GraphQL transport and documents of the AniList social layer. AniList allows 90 requests per
// minute; a 429 is retried once after `Retry-After` (capped), then surfaces as AppError.RateLimited("AniList").

class AniListSocialApi(
    private val client: OkHttpClient,
    private val endpoint: String = SocialRepositoryImpl.GRAPHQL_URL,
    private val io: CoroutineDispatcher = Dispatchers.IO,
    private val maxRetryWaitSec: Long = MAX_RETRY_WAIT_SEC,
) {
    private val json = Json { ignoreUnknownKeys = true }

    /** POSTs [query]; returns `data`. A GraphQL `errors[]` without data maps to AppError.Http(status). [token] null = anonymous (public data). */
    suspend fun post(query: String, variables: JsonObject, token: String?): JsonObject {
        var attempt = 0
        while (true) {
            val (code, retryAfter, text) = withContext(io) { execute(query, variables, token) }
            if (code == 429) {
                val wait = retryAfter ?: DEFAULT_RETRY_SEC
                if (attempt == 0 && wait <= maxRetryWaitSec) {
                    attempt++
                    delay(wait * 1000)
                    continue
                }
                throw AppException(AppError.RateLimited("AniList"))
            }
            val root = runCatching { json.parseToJsonElement(text) as? JsonObject }.getOrNull()
            val data = root?.obj("data")
            val errors = (root?.get("errors") as? JsonArray).orEmpty().mapNotNull { it as? JsonObject }
            if (code !in 200..299 || (data == null && errors.isNotEmpty())) {
                val status = errors.firstNotNullOfOrNull { it.int("status") } ?: code.takeIf { it !in 200..299 } ?: 400
                if (status == 429) throw AppException(AppError.RateLimited("AniList"))
                throw AppException(AppError.Http(status))
            }
            return data ?: throw AppException(AppError.Http(502))
        }
    }

    private fun execute(query: String, variables: JsonObject, token: String?): Triple<Int, Long?, String> {
        val body = buildJsonObject {
            put("query", query)
            put("variables", variables)
        }.toString().toRequestBody(JSON_TYPE)
        val request = Request.Builder().url(endpoint).post(body)
            .header("Accept", "application/json")
            .apply { if (token != null) header("Authorization", "Bearer $token") }
            .build()
        try {
            client.newCall(request).execute().use { response ->
                val retry = response.header("Retry-After")?.trim()?.toLongOrNull()
                return Triple(response.code, retry, response.body.string())
            }
        } catch (e: IOException) {
            throw AppException(AppError.Offline, e)
        }
    }

    companion object {
        const val MAX_RETRY_WAIT_SEC: Long = 65
        const val DEFAULT_RETRY_SEC: Long = 60
        private val JSON_TYPE = "application/json; charset=utf-8".toMediaType()

        const val USER_FIELDS = "id name avatar { large medium } siteUrl"
        const val MEDIA_FIELDS = "id idMal type format status season seasonYear episodes averageScore genres bannerImage " +
            "title { userPreferred romaji english native } coverImage { large medium color }"

        private const val PROFILE_FIELDS = "id name about(asHtml: false) bannerImage siteUrl isFollowing isFollower avatar { large medium } " +
            "statistics { anime { count episodesWatched minutesWatched meanScore } manga { count chaptersRead } } " +
            "favourites { anime(perPage: 12) { nodes { $MEDIA_FIELDS } } manga(perPage: 12) { nodes { $MEDIA_FIELDS } } " +
            "characters(perPage: 12) { nodes { id name { userPreferred full } image { large medium } } } }"

        const val VIEWER = "query { Viewer { $PROFILE_FIELDS unreadNotificationCount } }"
        const val USER = "query (\$name: String) { User(name: \$name) { $PROFILE_FIELDS } }"
        const val FOLLOW_COUNTS = "query (\$id: Int!) { followers: Page(perPage: 1) { pageInfo { total } followers(userId: \$id) { id } } " +
            "following: Page(perPage: 1) { pageInfo { total } following(userId: \$id) { id } } }"
        const val FOLLOWERS = "query (\$id: Int!, \$page: Int) { Page(page: \$page, perPage: 30) { pageInfo { currentPage hasNextPage } " +
            "followers(userId: \$id, sort: [USERNAME]) { $USER_FIELDS isFollowing isFollower } } }"
        const val FOLLOWING = "query (\$id: Int!, \$page: Int) { Page(page: \$page, perPage: 30) { pageInfo { currentPage hasNextPage } " +
            "following(userId: \$id, sort: [USERNAME]) { $USER_FIELDS isFollowing isFollower } } }"
        const val TOGGLE_FOLLOW = "mutation (\$id: Int) { ToggleFollow(userId: \$id) { id isFollowing } }"

        private const val ACTIVITY_FIELDS =
            "... on ListActivity { id status progress createdAt likeCount isLiked replyCount siteUrl user { $USER_FIELDS } media { $MEDIA_FIELDS } } " +
                "... on TextActivity { id text(asHtml: false) createdAt likeCount isLiked replyCount siteUrl user { $USER_FIELDS } } " +
                "... on MessageActivity { id message(asHtml: false) createdAt likeCount isLiked replyCount siteUrl " +
                "messenger { $USER_FIELDS } recipient { $USER_FIELDS } }"

        const val ACTIVITIES = "query (\$userId: Int, \$following: Boolean, \$mediaId: Int, \$page: Int) { Page(page: \$page, perPage: 25) { " +
            "pageInfo { currentPage hasNextPage } activities(userId: \$userId, isFollowing: \$following, mediaId: \$mediaId, sort: [ID_DESC]) { " +
            "$ACTIVITY_FIELDS } } }"
        const val ACTIVITY = "query (\$id: Int) { Activity(id: \$id) { $ACTIVITY_FIELDS } }"
        const val REPLIES = "query (\$id: Int, \$page: Int) { Page(page: \$page, perPage: 50) { pageInfo { currentPage hasNextPage } " +
            "activityReplies(activityId: \$id) { id text(asHtml: false) createdAt likeCount isLiked user { $USER_FIELDS } } } }"
        const val TOGGLE_LIKE = "mutation (\$id: Int) { ToggleLikeV2(id: \$id, type: ACTIVITY) { " +
            "... on ListActivity { id isLiked } ... on TextActivity { id isLiked } ... on MessageActivity { id isLiked } } }"
        const val SAVE_REPLY = "mutation (\$id: Int, \$text: String) { SaveActivityReply(activityId: \$id, text: \$text) { " +
            "id text(asHtml: false) createdAt likeCount isLiked user { $USER_FIELDS } } }"
        const val SAVE_TEXT = "mutation (\$text: String) { SaveTextActivity(text: \$text) { " +
            "id text(asHtml: false) createdAt likeCount isLiked replyCount siteUrl user { $USER_FIELDS } } }"

        const val UNREAD = "query { Viewer { id unreadNotificationCount } }"
        const val NOTIFICATIONS = "query (\$page: Int, \$reset: Boolean) { Page(page: \$page, perPage: 25) { pageInfo { currentPage hasNextPage } " +
            "notifications(resetNotificationCount: \$reset) { " +
            "... on AiringNotification { id type episode contexts createdAt media { $MEDIA_FIELDS } } " +
            "... on FollowingNotification { id type context createdAt user { $USER_FIELDS } } " +
            "... on ActivityMessageNotification { id type context createdAt activityId user { $USER_FIELDS } } " +
            "... on ActivityMentionNotification { id type context createdAt activityId user { $USER_FIELDS } } " +
            "... on ActivityReplyNotification { id type context createdAt activityId user { $USER_FIELDS } } " +
            "... on ActivityReplySubscribedNotification { id type context createdAt activityId user { $USER_FIELDS } } " +
            "... on ActivityLikeNotification { id type context createdAt activityId user { $USER_FIELDS } } " +
            "... on ActivityReplyLikeNotification { id type context createdAt activityId user { $USER_FIELDS } } " +
            "... on ThreadCommentMentionNotification { id type context createdAt user { $USER_FIELDS } } " +
            "... on ThreadCommentReplyNotification { id type context createdAt user { $USER_FIELDS } } " +
            "... on ThreadCommentSubscribedNotification { id type context createdAt user { $USER_FIELDS } } " +
            "... on ThreadCommentLikeNotification { id type context createdAt user { $USER_FIELDS } } " +
            "... on ThreadLikeNotification { id type context createdAt user { $USER_FIELDS } } " +
            "... on RelatedMediaAdditionNotification { id type context createdAt media { $MEDIA_FIELDS } } " +
            "... on MediaDataChangeNotification { id type context createdAt media { $MEDIA_FIELDS } } " +
            "... on MediaMergeNotification { id type context createdAt media { $MEDIA_FIELDS } } " +
            "... on MediaDeletionNotification { id type context createdAt deletedMediaTitle } } } }"
        const val REVIEWS = "query (\$mediaId: Int, \$page: Int) { Page(page: \$page, perPage: 10) { pageInfo { currentPage hasNextPage } " +
            "reviews(mediaId: \$mediaId, sort: [RATING_DESC]) { id summary body(asHtml: false) score rating ratingAmount createdAt siteUrl " +
            "user { $USER_FIELDS } } } }"
    }
}
