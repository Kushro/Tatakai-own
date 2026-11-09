// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.StateFlow

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.7, plan 14 §0.1 #10): social = ONLY what AniList offers
// (profile, followers / following, activity feed, likes / replies / status posts, notifications by polling, reviews of a title). No own
// backend, no tables, no comments, no rooms. Desktop counterpart: tk-core/social/** + components/reviews.rs (worker n). Signatures FINAL;
// bodies: worker t (:core:social `SocialRepositoryImpl`, bound in `socialModule`; UI in :feature:social). The AniList access token comes
// from `ListsRepository.accessToken(ListProvider.ANILIST)` (q's OAuth of wave A); without a linked AniList account every call fails with
// `AppException(AppError.Unknown(SOCIAL_NOT_LINKED))` and the screens show "Conecta AniList" -> Settings › Listas y cuentas.

/** Error code (`AppError.Unknown(code)`) when no AniList account is linked. */
const val SOCIAL_NOT_LINKED: String = "social:not_linked"

data class SocialStats(
    val animeCount: Int = 0,
    val episodesWatched: Int = 0,
    val minutesWatched: Long = 0,
    val meanScore: Float? = null,
    val mangaCount: Int = 0,
    val chaptersRead: Int = 0,
)

/** AniList `User`. [isFollowing] = the viewer follows them; [isFollower] = they follow the viewer. */
data class SocialUser(
    val id: Int,
    val name: String,
    val avatar: String? = null,
    val banner: String? = null,
    /** Markdown-ish `about`, already converted to plain text. */
    val about: String? = null,
    val followers: Int = 0,
    val following: Int = 0,
    val isFollowing: Boolean = false,
    val isFollower: Boolean = false,
    val isViewer: Boolean = false,
    val siteUrl: String,
    val stats: SocialStats? = null,
    val favoriteAnime: List<Media> = emptyList(),
    val favoriteManga: List<Media> = emptyList(),
    val favoriteCharacters: List<CharacterRef> = emptyList(),
)

data class SocialPage<T>(val items: List<T>, val page: Int, val hasNext: Boolean)

/** AniList `ActivityUnion`. */
sealed interface SocialActivity {
    val id: Int
    val user: SocialUser
    val createdAtSec: Long
    val likes: Int
    val liked: Boolean
    val replyCount: Int
    val siteUrl: String

    /** `ListActivity`: "watched episode 5 of X" / "read chapter 12 of Y" / "plans to watch Z". */
    data class ListActivity(
        override val id: Int,
        override val user: SocialUser,
        override val createdAtSec: Long,
        override val likes: Int,
        override val liked: Boolean,
        override val replyCount: Int,
        override val siteUrl: String,
        val media: Media,
        /** AniList `status` text (`watched episode`, `read chapter`, `plans to watch`...). */
        val status: String,
        /** `5` / `1 - 3`; null for status-only entries. */
        val progress: String?,
    ) : SocialActivity

    /** `TextActivity` (a status post) and `MessageActivity` (a message on someone's profile; [recipient] non-null). */
    data class TextActivity(
        override val id: Int,
        override val user: SocialUser,
        override val createdAtSec: Long,
        override val likes: Int,
        override val liked: Boolean,
        override val replyCount: Int,
        override val siteUrl: String,
        val text: String,
        val recipient: SocialUser? = null,
    ) : SocialActivity
}

data class ActivityReply(val id: Int, val user: SocialUser, val text: String, val createdAtSec: Long, val likes: Int, val liked: Boolean)

/** AniList `NotificationUnion` flattened; [type] is the AniList `NotificationType` name (`AIRING`, `FOLLOWING`, `ACTIVITY_LIKE`...). */
data class SocialNotification(
    val id: Int,
    val type: String,
    val createdAtSec: Long,
    /** Ready-to-show sentence built from the AniList `contexts` (localised by t). */
    val text: String,
    val media: Media? = null,
    val user: SocialUser? = null,
    /** AIRING: episode number. */
    val episode: Int? = null,
    /** Activity id to open (likes / replies / mentions). */
    val activityId: Int? = null,
    val unread: Boolean = false,
)

/** AniList `Review` of a title (checklist `details.reviews`). */
data class Review(
    val id: Int,
    val user: SocialUser,
    /** 0..100. */
    val score: Int,
    val summary: String,
    /** Full body, plain text; loaded with the summary (AniList returns both). */
    val body: String,
    /** Up-votes vs total ratings. */
    val rating: Int,
    val ratingAmount: Int,
    val createdAtSec: Long,
    val siteUrl: String,
)

/**
 * Implemented by worker t (`SocialRepositoryImpl`, :core:social) over `graphql.anilist.co` with the viewer's token. Reads are cached
 * briefly (memory, 60 s); writes (`ToggleFollow`, `ToggleLikeV2`, `SaveActivityReply`, `SaveTextActivity`) go straight to AniList.
 * [unreadCount] is refreshed by a periodic worker (15 min, `AppSettings.socialNotifications`) and when the app comes to the foreground.
 */
interface SocialRepository {
    /** The linked AniList user (null = not linked); follows `ListsRepository.accounts`. */
    val viewer: StateFlow<SocialUser?>

    /** AniList `Viewer.unreadNotificationCount`. */
    val unreadCount: StateFlow<Int>

    /** [userName] null = the viewer's own profile. */
    suspend fun profile(userName: String? = null): SocialUser?

    suspend fun followers(userId: Int, page: Int = 1): SocialPage<SocialUser>

    suspend fun following(userId: Int, page: Int = 1): SocialPage<SocialUser>

    /** Returns the new `isFollowing`. */
    suspend fun toggleFollow(userId: Int): Result<Boolean>

    /** [userId] null = the viewer's feed; [followingOnly] = activity of the people the viewer follows (AniList `isFollowing: true`). */
    suspend fun activity(userId: Int? = null, followingOnly: Boolean = false, page: Int = 1): SocialPage<SocialActivity>

    suspend fun replies(activityId: Int, page: Int = 1): SocialPage<ActivityReply>

    /** Returns the new `liked`. */
    suspend fun toggleLike(activityId: Int): Result<Boolean>

    suspend fun reply(activityId: Int, text: String): Result<ActivityReply>

    suspend fun postStatus(text: String): Result<SocialActivity>

    /** [markRead] resets the unread counter on AniList (`resetNotificationCount: true`). */
    suspend fun notifications(page: Int = 1, markRead: Boolean = false): SocialPage<SocialNotification>

    suspend fun refreshUnread()

    /** Reviews of a title, most helpful first (Details slot of t). */
    suspend fun reviews(ref: MediaRef, page: Int = 1): SocialPage<Review>

    /** The viewer's own activity on this title (progress posts), newest first (Details slot of t). */
    suspend fun mediaActivity(ref: MediaRef): List<SocialActivity>
}
