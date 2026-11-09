// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import app.tatakai.next.model.CharacterEdge
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.Episode
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.NextAiring
import app.tatakai.next.model.StaffRef
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.ui.media.alternativeTitle
import app.tatakai.next.ui.media.displayTitle
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class DetailsLogicTest {
    private fun ep(n: Int, title: String? = null, air: String? = null) = Episode(n, title, null, air)

    private val media = Media(
        ref = MediaRef(anilistId = 154587), kind = MediaKind.ANIME, title = "Sousou no Frieren",
        titles = mapOf("romaji" to "Sousou no Frieren", "english" to "Frieren: Beyond Journey's End", "native" to "葬送のフリーレン"),
        cover = null, banner = null, color = null, synopsis = null, genres = emptyList(), episodes = 28, status = null, season = null,
        year = null, score = null, sourceProvider = "anilist",
    )

    @Test
    fun `unaired placeholders reach the next airing episode and stop at the total`() {
        val items = buildEpisodeItems(listOf(ep(1), ep(2)), NextAiring(episode = 4, airingAtEpochSec = 2_000), total = 3, nowSec = 1_000)
        assertEquals(listOf(1, 2, 3), items.map { it.episode.number })
        assertEquals(listOf(false, false, false), items.map { it.unaired })

        val open = buildEpisodeItems(listOf(ep(1), ep(2)), NextAiring(episode = 3, airingAtEpochSec = 2_000), total = null, nowSec = 1_000)
        assertEquals(listOf(1, 2, 3), open.map { it.episode.number })
        assertEquals(listOf(false, false, true), open.map { it.unaired })
    }

    @Test
    fun `a future air date marks the episode unaired`() {
        val items = buildEpisodeItems(listOf(ep(1, air = "2020-01-01"), ep(2, air = "2999-01-01T00:00:00Z")), null, null, nowSec = 1_700_000_000)
        assertEquals(listOf(false, true), items.map { it.unaired })
    }

    @Test
    fun `episode search matches number prefixes or titles`() {
        val items = listOf(ep(1, "The Journey's End"), ep(10, "Frieren"), ep(12, "Mimic")).map { EpisodeItem(it, false) }
        assertEquals(listOf(1, 10, 12), filterEpisodes(items, "1").map { it.episode.number })
        assertEquals(listOf(12), filterEpisodes(items, "12").map { it.episode.number })
        assertEquals(listOf(10), filterEpisodes(items, "frie").map { it.episode.number })
        assertEquals(3, filterEpisodes(items, "  ").size)
    }

    @Test
    fun `countdown splits days hours and minutes`() {
        assertEquals(Countdown(1, 2, 3), countdown(86_400 + 2 * 3_600 + 3 * 60 + 59, 0))
        assertNull(countdown(100, 100))
    }

    @Test
    fun `title language picks the matching title or falls back`() {
        assertEquals("Frieren: Beyond Journey's End", media.displayTitle(TitleLanguage.ENGLISH))
        assertEquals("葬送のフリーレン", media.displayTitle(TitleLanguage.NATIVE))
        assertEquals("Sousou no Frieren", media.copy(titles = emptyMap()).displayTitle(TitleLanguage.ENGLISH))
        assertEquals("葬送のフリーレン", media.alternativeTitle("Sousou no Frieren"))
    }

    @Test
    fun `relations sort the story chain first, voice actor prefers japanese, share link points to AniList`() {
        val r = listOf("ADAPTATION", "CHARACTER", "SEQUEL", "PREQUEL").map { MediaRelation(it, media) }
        assertEquals(listOf("PREQUEL", "SEQUEL", "ADAPTATION", "CHARACTER"), sortRelations(r).map { it.relation })
        val edge = CharacterEdge(
            CharacterRef(1, "Frieren"),
            voiceActors = listOf(StaffRef(2, "Mallorie Rodak", language = "ENGLISH"), StaffRef(3, "Atsumi Tanezaki", language = "JAPANESE")),
        )
        assertEquals("Atsumi Tanezaki", preferredVoiceActor(edge)?.name)
        assertEquals("https://anilist.co/anime/154587", shareLink(media))
        assertEquals("https://myanimelist.net/anime/52991", shareLink(media.copy(ref = MediaRef(malId = 52991))))
    }
}
