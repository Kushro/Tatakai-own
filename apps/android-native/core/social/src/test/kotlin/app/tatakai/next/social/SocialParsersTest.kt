// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.SocialActivity
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave B (16 §5 t.5g, worker t): AniList User / ActivityUnion / NotificationUnion / Review parsers on recorded answers.
class SocialParsersTest {
    private fun fixture(name: String): JsonObject =
        (Json.parseToJsonElement(checkNotNull(javaClass.getResource("/anilist/$name")).readText()) as JsonObject)["data"] as JsonObject

    @Test
    fun viewerProfile() {
        val u = SocialParsers.user(fixture("viewer.json").obj("Viewer"), viewerId = 5001, followers = 10, following = 4)
        checkNotNull(u)
        assertEquals("Octa", u.name)
        assertTrue(u.isViewer)
        assertEquals(10, u.followers)
        assertEquals("https://s4.anilist.co/a.png", u.avatar)
        assertEquals("Hi there spoiler link\nline two", u.about)
        assertEquals(120, u.stats?.animeCount)
        assertEquals(57600L, u.stats?.minutesWatched)
        assertEquals(78.5f, u.stats?.meanScore)
        assertEquals(340, u.stats?.chaptersRead)
        val fav = u.favoriteAnime.single()
        assertEquals(154587, fav.ref.anilistId)
        assertEquals(52991, fav.ref.malId)
        assertEquals(MediaKind.ANIME, fav.kind)
        assertEquals("Frieren: Beyond Journey's End", fav.titles["english"])
        assertEquals("Frieren", u.favoriteCharacters.single().name)
    }

    @Test
    fun activityUnionAndPaging() {
        val page = SocialParsers.page(fixture("activities.json"), "activities", 1) { SocialParsers.activity(it, viewerId = 5001) }
        assertTrue(page.hasNext)
        assertEquals(3, page.items.size)
        val list = assertIs<SocialActivity.ListActivity>(page.items[0])
        assertEquals("watched episode", list.status)
        assertEquals("5", list.progress)
        assertTrue(list.liked)
        assertTrue(list.user.isViewer)
        assertEquals("Sousou no Frieren", list.media.title)
        val text = assertIs<SocialActivity.TextActivity>(page.items[1])
        assertEquals("Hello & welcome\nto my feed", text.text)
        assertNull(text.recipient)
        val message = assertIs<SocialActivity.TextActivity>(page.items[2])
        assertEquals("Friend", message.user.name)
        assertEquals("Octa", message.recipient?.name)
    }

    @Test
    fun notificationsAreLocalisedFromTheirType() {
        val data = fixture("notifications.json")
        var i = 0
        val page = SocialParsers.page(data, "notifications", 1) { SocialParsers.notification(it, 5001, EnglishSocialTexts, unread = i++ < 2) }
        val n = page.items
        assertEquals(5, n.size)
        assertFalse(page.hasNext)
        assertEquals("Episode 12 of Sousou no Frieren aired.", n[0].text)
        assertEquals(12, n[0].episode)
        assertTrue(n[0].unread)
        assertEquals("Friend started following you.", n[1].text)
        assertEquals(901, n[2].activityId)
        assertFalse(n[2].unread)
        assertEquals("Old Show was removed from AniList.", n[3].text)
        // Unknown type: AniList's own sentence.
        assertEquals("Other did something.", n[4].text)
    }

    @Test
    fun spanishTemplatesFillTheSameArguments() {
        val es = mapOf("airing" to "Se emitió el episodio %1\$s de %2\$s.", "following" to "%1\$s empezó a seguirte.")
        assertEquals("Se emitió el episodio 3 de X.", localizedNotification(es::get, "AIRING", null, "X", 3, "fb"))
        assertEquals("Ana empezó a seguirte.", localizedNotification(es::get, "FOLLOWING", "Ana", null, null, "fb"))
        // A template the map lacks, or a missing argument, falls back to AniList's text.
        assertEquals("fb", localizedNotification(es::get, "ACTIVITY_LIKE", "Ana", null, null, "fb"))
        assertEquals("fb", localizedNotification(es::get, "AIRING", null, null, 3, "fb"))
    }

    @Test
    fun contextsJoinAroundEpisodeAndTitle() {
        assertEquals("Episode 7 of Show aired.", SocialParsers.contextText("AIRING", listOf("Episode ", " of ", " aired."), null, null, "Show", 7, null))
        assertEquals("Ana liked your activity.", SocialParsers.contextText("ACTIVITY_LIKE", emptyList(), " liked your activity.", "Ana", null, null, null))
    }

    @Test
    fun reviews() {
        val page = SocialParsers.page(fixture("reviews.json"), "reviews", 1) { SocialParsers.review(it, null) }
        val r = page.items.single()
        assertEquals(95, r.score)
        assertEquals("A quiet masterpiece", r.summary)
        assertEquals("Great pacing.\n\nWorth it.", r.body)
        assertEquals(120, r.rating)
        assertEquals("Critic", r.user.name)
        assertTrue(page.hasNext)
    }
}
