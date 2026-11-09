// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import app.tatakai.next.model.ActivityReply
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.Review
import app.tatakai.next.model.SOCIAL_NOT_LINKED
import app.tatakai.next.model.SocialActivity
import app.tatakai.next.model.SocialNotification
import app.tatakai.next.model.SocialPage
import app.tatakai.next.model.SocialRepository
import app.tatakai.next.model.SocialUser
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.OkHttpClient
import java.io.File

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.7, worker t; checklist social.*, details.reviews): [SocialRepository]
 * over `https://graphql.anilist.co` with `Authorization: Bearer <ListsRepository.accessToken(ANILIST)>`. Queries: `Viewer`, `User(name:)`
 * (+ `statistics`, `favourites`), `Page.followers / following`, `Page.activities(userId / isFollowing, sort: ID_DESC)`, `Page.activityReplies`,
 * `Page.notifications(resetNotificationCount:)`, `Viewer.unreadNotificationCount`, `Page.reviews(mediaId, sort: RATING_DESC)`; mutations
 * `ToggleFollow`, `ToggleLikeV2`, `SaveActivityReply`, `SaveTextActivity`. Reads cached 60 s in memory (+ the viewer in `cacheDir/viewer.json`
 * for the offline You card). `viewer` follows `ListsRepository.accounts` (null when AniList is not linked). Without a token every call
 * throws `AppException(AppError.Unknown(SOCIAL_NOT_LINKED))`. Rate limit (90 req/min) honoured with `Retry-After`.
 *
 * Stub: no viewer, zero unread, every call fails as "not linked".
 */
class SocialRepositoryImpl(
    private val client: OkHttpClient,
    private val lists: ListsRepository,
    private val cacheDir: File,
    private val notificationsEnabled: () -> Boolean = { true },
    private val clock: () -> Long = System::currentTimeMillis,
) : SocialRepository, AppDataWiper {
    private val _viewer = MutableStateFlow<SocialUser?>(null)
    override val viewer: StateFlow<SocialUser?> = _viewer.asStateFlow()
    private val _unread = MutableStateFlow(0)
    override val unreadCount: StateFlow<Int> = _unread.asStateFlow()

    override suspend fun profile(userName: String?): SocialUser? = notLinked()

    override suspend fun followers(userId: Int, page: Int): SocialPage<SocialUser> = notLinked()

    override suspend fun following(userId: Int, page: Int): SocialPage<SocialUser> = notLinked()

    override suspend fun toggleFollow(userId: Int): Result<Boolean> = Result.failure(notLinkedException())

    override suspend fun activity(userId: Int?, followingOnly: Boolean, page: Int): SocialPage<SocialActivity> = notLinked()

    override suspend fun replies(activityId: Int, page: Int): SocialPage<ActivityReply> = notLinked()

    override suspend fun toggleLike(activityId: Int): Result<Boolean> = Result.failure(notLinkedException())

    override suspend fun reply(activityId: Int, text: String): Result<ActivityReply> = Result.failure(notLinkedException())

    override suspend fun postStatus(text: String): Result<SocialActivity> = Result.failure(notLinkedException())

    override suspend fun notifications(page: Int, markRead: Boolean): SocialPage<SocialNotification> = notLinked()

    override suspend fun refreshUnread() {}

    override suspend fun reviews(ref: MediaRef, page: Int): SocialPage<Review> = notLinked()

    override suspend fun mediaActivity(ref: MediaRef): List<SocialActivity> = emptyList()

    /** "Restablecer la app": cached viewer and pages. */
    override suspend fun wipe() {
        _viewer.value = null
        _unread.value = 0
        cacheDir.deleteRecursively()
    }

    private fun notLinkedException() = AppException(AppError.Unknown(SOCIAL_NOT_LINKED))

    private fun <T> notLinked(): T = throw notLinkedException()

    companion object {
        const val CACHE_DIR: String = "social"
        const val GRAPHQL_URL: String = "https://graphql.anilist.co"

        /** Notification channel of [SocialNotificationsWorker]. */
        const val CHANNEL_ID: String = "social"
    }
}
