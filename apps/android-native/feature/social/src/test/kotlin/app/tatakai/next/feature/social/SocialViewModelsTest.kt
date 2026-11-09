// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.social

import app.tatakai.next.model.ActivityReply
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.Review
import app.tatakai.next.model.SOCIAL_NOT_LINKED
import app.tatakai.next.model.SocialActivity
import app.tatakai.next.model.SocialNotification
import app.tatakai.next.model.SocialPage
import app.tatakai.next.model.SocialRepository
import app.tatakai.next.model.SocialUser
import app.tatakai.next.social.SocialActivityLookup
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import java.io.IOException
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

// F6 (parity wave B tests, 16 §5 t / §6): view models of the AniList social screens over a fake SocialRepository.
@OptIn(ExperimentalCoroutinesApi::class)
class SocialViewModelsTest {
    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private val me = SocialUser(id = 1, name = "Me", siteUrl = "https://anilist.co/user/Me", isViewer = true, followers = 3)
    private val friend = SocialUser(id = 7, name = "Friend", siteUrl = "https://anilist.co/user/Friend", followers = 10)
    private val media = Media(
        ref = MediaRef(anilistId = 99), kind = MediaKind.ANIME, title = "Show", titles = emptyMap(), cover = null, banner = null, color = null,
        synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null, sourceProvider = "anilist",
    )

    private fun text(id: Int, liked: Boolean = false, likes: Int = 0, replies: Int = 0) =
        SocialActivity.TextActivity(id, me, createdAtSec = id.toLong(), likes = likes, liked = liked, replyCount = replies, siteUrl = "", text = "t$id")

    private class ActivityCall(val userId: Int?, val followingOnly: Boolean, val page: Int)

    private class FakeSocial(
        var profiles: Map<String?, SocialUser?> = emptyMap(),
        var profileError: Exception? = null,
    ) : SocialRepository {
        override val viewer = MutableStateFlow<SocialUser?>(null)
        override val unreadCount = MutableStateFlow(0)
        val activityCalls = mutableListOf<ActivityCall>()
        var activityPages: (Int) -> SocialPage<SocialActivity> = { p -> SocialPage(emptyList(), p, false) }
        val followerPages = mutableListOf<Int>()
        var followResult: Result<Boolean> = Result.success(true)
        var followCalls = 0
        var likeResult: (Int) -> Result<Boolean> = { Result.success(true) }
        var postResult: Result<SocialActivity>? = null
        var replyResult: Result<ActivityReply>? = null
        val notificationCalls = mutableListOf<Pair<Int, Boolean>>()
        var refreshUnreadCalls = 0
        var reviewPages: (Int) -> SocialPage<Review> = { p -> SocialPage(emptyList(), p, false) }
        var mediaActivityResult: () -> List<SocialActivity> = { emptyList() }

        override suspend fun profile(userName: String?): SocialUser? {
            profileError?.let { throw it }
            return profiles[userName]
        }

        override suspend fun followers(userId: Int, page: Int): SocialPage<SocialUser> {
            followerPages += page
            return SocialPage(listOf(SocialUser(id = 100 + page, name = "f$page", siteUrl = "")), page, hasNext = page < 2)
        }

        override suspend fun following(userId: Int, page: Int): SocialPage<SocialUser> = SocialPage(emptyList(), page, false)

        override suspend fun toggleFollow(userId: Int): Result<Boolean> {
            followCalls++
            return followResult
        }

        override suspend fun activity(userId: Int?, followingOnly: Boolean, page: Int): SocialPage<SocialActivity> {
            activityCalls += ActivityCall(userId, followingOnly, page)
            return activityPages(page)
        }

        override suspend fun replies(activityId: Int, page: Int): SocialPage<ActivityReply> = SocialPage(emptyList(), page, false)

        override suspend fun toggleLike(activityId: Int): Result<Boolean> = likeResult(activityId)

        override suspend fun reply(activityId: Int, text: String): Result<ActivityReply> = replyResult ?: Result.failure(IOException("offline"))

        override suspend fun postStatus(text: String): Result<SocialActivity> = postResult ?: Result.failure(IOException("offline"))

        override suspend fun notifications(page: Int, markRead: Boolean): SocialPage<SocialNotification> {
            notificationCalls += page to markRead
            return SocialPage(listOf(SocialNotification(page, "AIRING", 0, "n$page")), page, hasNext = page < 3)
        }

        override suspend fun refreshUnread() {
            refreshUnreadCalls++
        }

        override suspend fun reviews(ref: MediaRef, page: Int): SocialPage<Review> = reviewPages(page)

        override suspend fun mediaActivity(ref: MediaRef): List<SocialActivity> = mediaActivityResult()
    }

    // ---- profile ---------------------------------------------------------------------------------------------------------------------

    @Test
    fun ownProfileLoadsAndRefreshesTheActivityOfTheUser() = runTest {
        val repo = FakeSocial(profiles = mapOf(null to me))
        repo.activityPages = { p -> SocialPage(listOf(text(p)), p, hasNext = false) }
        val vm = ProfileViewModel(userName = null, repo = repo)
        assertEquals(LoadState.Ok(me), vm.state.value.user)
        assertEquals(1, repo.activityCalls.size)
        assertEquals(me.id, repo.activityCalls.single().userId)
        assertFalse(repo.activityCalls.single().followingOnly)
        assertEquals(listOf(1), vm.activity.state.value.items.map { it.id })
        assertFalse(vm.activity.state.value.loading)
    }

    @Test
    fun followingOnlyAsksForTheFollowedFeedWithoutUserId() = runTest {
        val repo = FakeSocial(profiles = mapOf(null to me))
        val vm = ProfileViewModel(userName = null, repo = repo)
        vm.setFollowingOnly(true)
        val last = repo.activityCalls.last()
        assertNull(last.userId)
        assertTrue(last.followingOnly)
        assertEquals(1, last.page)
        val before = repo.activityCalls.size
        vm.setFollowingOnly(true)
        assertEquals(before, repo.activityCalls.size, "same value does not reload")
    }

    @Test
    fun missingProfileIs404AndNotLinkedIsRecognised() = runTest {
        val missing = ProfileViewModel(userName = "ghost", repo = FakeSocial(profiles = emptyMap()))
        assertEquals(LoadState.Err(AppError.Http(404)), missing.state.value.user)

        val unlinked = ProfileViewModel(null, FakeSocial(profileError = AppException(AppError.Unknown(SOCIAL_NOT_LINKED))))
        val err = assertIs<LoadState.Err>(unlinked.state.value.user)
        assertTrue(err.cause.isNotLinked())

        val offline = ProfileViewModel(null, FakeSocial(profileError = IOException("down")))
        assertEquals(LoadState.Err(AppError.Offline), offline.state.value.user)
    }

    @Test
    fun toggleFollowUpdatesTheCountAndReportsFailures() = runTest {
        val repo = FakeSocial(profiles = mapOf("Friend" to friend))
        val vm = ProfileViewModel("Friend", repo)
        vm.toggleFollow()
        val now = assertIs<LoadState.Ok<SocialUser>>(vm.state.value.user).value
        assertTrue(now.isFollowing)
        assertEquals(11, now.followers)
        assertFalse(vm.state.value.followBusy)

        repo.followResult = Result.failure(IOException("offline"))
        vm.toggleFollow()
        assertEquals(SocialNotice.Failed(AppError.Offline), vm.notices.first())
        assertFalse(vm.state.value.followBusy)
        assertTrue(assertIs<LoadState.Ok<SocialUser>>(vm.state.value.user).value.isFollowing, "failure keeps the last known state")
    }

    @Test
    fun viewerCannotFollowThemselves() = runTest {
        val repo = FakeSocial(profiles = mapOf(null to me))
        val vm = ProfileViewModel(null, repo)
        vm.toggleFollow()
        assertEquals(0, repo.followCalls)
    }

    @Test
    fun followersTabLoadsLazilyAndPaginates() = runTest {
        val repo = FakeSocial(profiles = mapOf("Friend" to friend))
        val vm = ProfileViewModel("Friend", repo)
        assertTrue(repo.followerPages.isEmpty())
        vm.selectTab(ProfileTab.FOLLOWERS)
        assertEquals(ProfileTab.FOLLOWERS, vm.state.value.tab)
        assertEquals(listOf(1), repo.followerPages)
        vm.selectTab(ProfileTab.FOLLOWERS)
        assertEquals(listOf(1), repo.followerPages, "an already loaded tab is not reloaded")
        vm.followers.loadMore()
        assertEquals(listOf(1, 2), repo.followerPages)
        assertEquals(listOf(101, 102), vm.followers.state.value.items.map { it.id })
        assertFalse(vm.followers.state.value.hasNext)
        vm.followers.loadMore()
        assertEquals(listOf(1, 2), repo.followerPages, "no page after the last one")
    }

    @Test
    fun likeIsOptimisticAndRolledBackOnFailure() = runTest {
        val repo = FakeSocial(profiles = mapOf(null to me))
        repo.activityPages = { p -> SocialPage(listOf(text(1, likes = 2), text(2, likes = 0)), p, false) }
        val vm = ProfileViewModel(null, repo)

        vm.toggleLike(1)
        val liked = vm.activity.state.value.items.first { it.id == 1 }
        assertTrue(liked.liked)
        assertEquals(3, liked.likes)

        repo.likeResult = { Result.failure(IOException("offline")) }
        vm.toggleLike(2)
        val rolled = vm.activity.state.value.items.first { it.id == 2 }
        assertFalse(rolled.liked)
        assertEquals(0, rolled.likes)
        assertEquals(SocialNotice.Failed(AppError.Offline), vm.notices.first())
    }

    @Test
    fun postedStatusGoesOnTop() = runTest {
        val repo = FakeSocial(profiles = mapOf(null to me))
        repo.activityPages = { p -> SocialPage(listOf(text(1), text(2)), p, false) }
        repo.postResult = Result.success(text(50))
        val vm = ProfileViewModel(null, repo)
        vm.postStatus("   ")
        assertEquals(listOf(1, 2), vm.activity.state.value.items.map { it.id }, "blank text is ignored")
        vm.postStatus("hello")
        assertEquals(listOf(50, 1, 2), vm.activity.state.value.items.map { it.id })
        assertEquals(SocialNotice.Posted, vm.notices.first())
        assertFalse(vm.state.value.posting)
    }

    @Test
    fun pagerErrorStopsLoadMoreUntilRetry() = runTest {
        val repo = FakeSocial(profiles = mapOf(null to me))
        var fail = true
        repo.activityPages = { p -> if (fail) throw IOException("down") else SocialPage(listOf(text(p)), p, true) }
        val vm = ProfileViewModel(null, repo)
        val s = vm.activity.state.value
        assertEquals(AppError.Offline, s.initialError)
        assertFalse(s.loading)
        val calls = repo.activityCalls.size
        vm.activity.loadMore()
        assertEquals(calls, repo.activityCalls.size, "loadMore is blocked while an error is shown")
        fail = false
        vm.activity.retry()
        assertEquals(1, repo.activityCalls.last().page, "retry with no items reloads the first page")
        assertEquals(listOf(1), vm.activity.state.value.items.map { it.id })
        assertNull(vm.activity.state.value.error)
    }

    // ---- thread ---------------------------------------------------------------------------------------------------------------------

    @Test
    fun threadLoadsFromLookupAndReplyBumpsTheCount() = runTest {
        val repo = FakeSocial()
        val reply = ActivityReply(id = 5, user = me, text = "nice", createdAtSec = 1, likes = 0, liked = false)
        repo.replyResult = Result.success(reply)
        val vm = ActivityThreadViewModel(3, repo, SocialActivityLookup { id -> if (id == 3) text(3, replies = 1) else null })
        assertEquals(3, assertIs<LoadState.Ok<SocialActivity>>(vm.state.value.activity).value.id)
        vm.reply("nice")
        assertEquals(listOf(5), vm.replies.state.value.items.map { it.id })
        assertEquals(2, assertIs<LoadState.Ok<SocialActivity>>(vm.state.value.activity).value.replyCount)
        assertEquals(SocialNotice.Replied, vm.notices.first())
        assertFalse(vm.state.value.sending)
    }

    @Test
    fun threadWithoutLookupIs404AndLikeRollsBack() = runTest {
        val repo = FakeSocial()
        val missing = ActivityThreadViewModel(3, repo, lookup = null)
        assertEquals(LoadState.Err(AppError.Http(404)), missing.state.value.activity)

        repo.likeResult = { Result.failure(AppException(AppError.RateLimited("anilist"))) }
        val vm = ActivityThreadViewModel(3, repo, SocialActivityLookup { text(3, likes = 4) })
        vm.toggleLike()
        val a = assertIs<LoadState.Ok<SocialActivity>>(vm.state.value.activity).value
        assertFalse(a.liked)
        assertEquals(4, a.likes)
        assertEquals(SocialNotice.Failed(AppError.RateLimited("anilist")), vm.notices.first())
    }

    // ---- notifications / slot -------------------------------------------------------------------------------------------------------

    @Test
    fun onlyTheFirstNotificationsPageMarksRead() = runTest {
        val repo = FakeSocial()
        val vm = NotificationsViewModel(repo)
        vm.pager.loadMore()
        assertEquals(listOf(1 to true, 2 to false), repo.notificationCalls)
        assertEquals(listOf(1, 2), vm.pager.state.value.items.map { it.id })
        vm.onResume()
        assertEquals(1, repo.refreshUnreadCalls)
        repo.unreadCount.value = 4
        assertEquals(4, vm.unread.value)
    }

    @Test
    fun slotShowsReviewsOwnActivityAndLinkedFlag() = runTest {
        val repo = FakeSocial()
        repo.viewer.value = me
        val review = Review(1, friend, score = 80, summary = "good", body = "long", rating = 3, ratingAmount = 4, createdAtSec = 0, siteUrl = "")
        repo.reviewPages = { p -> SocialPage(listOf(review), p, false) }
        repo.mediaActivityResult = { listOf(text(9)) }
        val vm = SocialSlotViewModel(anilistId = 99, repo = repo)
        assertEquals(listOf(1), vm.reviews.state.value.items.map { it.id })
        assertTrue(vm.state.value.linked)
        assertEquals(listOf(9), vm.state.value.ownActivity.map { it.id })

        val unlinked = FakeSocial().apply { mediaActivityResult = { throw AppException(AppError.Unknown(SOCIAL_NOT_LINKED)) } }
        val vm2 = SocialSlotViewModel(anilistId = 99, repo = unlinked)
        assertFalse(vm2.state.value.linked)
        assertTrue(vm2.state.value.ownActivity.isEmpty())
    }

    @Test
    fun notificationTargetPrefersActivityForActivityTypes() {
        val n = SocialNotification(1, "ACTIVITY_REPLY", 0, "x", media = media, user = friend, activityId = 4)
        assertEquals(NotificationTarget.OpenActivity(4), notificationTarget(n))
        assertEquals(NotificationTarget.OpenMedia(media), notificationTarget(n.copy(activityId = null)))
    }
}
