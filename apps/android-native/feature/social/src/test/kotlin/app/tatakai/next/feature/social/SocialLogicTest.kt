// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.social

import app.tatakai.next.model.AppError
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.SOCIAL_NOT_LINKED
import app.tatakai.next.model.SocialActivity
import app.tatakai.next.model.SocialNotification
import app.tatakai.next.model.SocialPage
import app.tatakai.next.model.SocialUser
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

// parity wave B (16 §3.7, worker t): pure logic of the social screens.
class SocialLogicTest {
    private val user = SocialUser(id = 7, name = "Friend", siteUrl = "https://anilist.co/user/Friend")
    private val media = Media(
        ref = MediaRef(anilistId = 1), kind = MediaKind.ANIME, title = "Show", titles = emptyMap(), cover = null, banner = null, color = null,
        synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null, sourceProvider = "anilist",
    )

    private fun n(type: String, media: Media? = null, user: SocialUser? = null, activityId: Int? = null) =
        SocialNotification(1, type, 0, "x", media = media, user = user, activityId = activityId)

    @Test
    fun notificationTargets() {
        assertEquals(NotificationTarget.OpenMedia(media), notificationTarget(n("AIRING", media = media)))
        assertEquals(NotificationTarget.OpenUser("Friend"), notificationTarget(n("FOLLOWING", user = user)))
        assertEquals(NotificationTarget.OpenActivity(9), notificationTarget(n("ACTIVITY_LIKE", user = user, activityId = 9)))
        assertEquals(NotificationTarget.OpenUser("Friend"), notificationTarget(n("THREAD_COMMENT_REPLY", user = user)))
        assertEquals(NotificationTarget.OpenMedia(media), notificationTarget(n("RELATED_MEDIA_ADDITION", media = media)))
        assertEquals(NotificationTarget.None, notificationTarget(n("MEDIA_DELETION")))
    }

    @Test
    fun likeToggleAdjustsTheCount() {
        val a = SocialActivity.TextActivity(1, user, 0, likes = 3, liked = false, replyCount = 0, siteUrl = "", text = "hi")
        val liked = a.withLike(true)
        assertTrue(liked.liked)
        assertEquals(4, liked.likes)
        assertEquals(3, liked.withLike(false).likes)
        assertEquals(a, a.withLike(false))
        assertEquals(0, a.copy(likes = 0, liked = true).withLike(false).likes)
        assertEquals(5, a.withReplies(5).replyCount)
    }

    @Test
    fun pagesAppendWithoutDuplicates() {
        val first = Paged<Int>().plus(SocialPage(listOf(1, 2, 3), 1, true)) { it }
        assertEquals(listOf(1, 2, 3), first.items)
        val second = first.plus(SocialPage(listOf(3, 4), 2, false)) { it }
        assertEquals(listOf(1, 2, 3, 4), second.items)
        assertFalse(second.hasNext)
        val refreshed = second.plus(SocialPage(listOf(9), 1, true)) { it }
        assertEquals(listOf(9), refreshed.items)
    }

    @Test
    fun notLinkedIsRecognised() {
        assertTrue(AppError.Unknown(SOCIAL_NOT_LINKED).isNotLinked())
        assertFalse(AppError.Offline.isNotLinked())
        assertFalse(AppError.Unknown("other").isNotLinked())
    }
}
