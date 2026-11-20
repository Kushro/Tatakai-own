// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import app.tatakai.next.model.CharacterEdge
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.Episode
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaArtwork
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
    fun `header backdrop prefers the ani zip fanart`() {
        val m = media.copy(banner = "b.jpg", cover = "c.jpg")
        assertEquals(HeaderBackdrop("f.jpg", lowRes = false), headerBackdrop(m, MediaArtwork(fanart = "f.jpg")))
        assertEquals(HeaderBackdrop("b.jpg", lowRes = true), headerBackdrop(m, MediaArtwork(clearlogo = "l.png")))
        assertEquals(HeaderBackdrop("c.jpg", lowRes = true), headerBackdrop(m.copy(banner = ""), null))
    }

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
    fun `earlier episodes fold so the list opens on the current one`() {
        val page = (1..12).map { EpisodeItem(ep(it), unaired = false) }
        assertEquals(3, hiddenLeading(page, current = 5), "1..3 fold, 4 stays as context")
        assertEquals(0, hiddenLeading(page, current = 4), "only 1..2 before 3: not worth folding")
        assertEquals(0, hiddenLeading(page, current = null))
        assertEquals(0, hiddenLeading(page, current = 40), "not on this page")
        val second = (101..200).map { EpisodeItem(ep(it), unaired = false) }
        assertEquals(49, hiddenLeading(second, current = 151))
        assertEquals(0, hiddenLeading(second, current = 101))
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

    private fun hist(ep: Int, sec: Int, dur: Int?, completed: Boolean = false) = app.tatakai.next.model.HistoryEntry(
        animeId = "1", title = "t", poster = null, episodeId = "e$ep", episode = ep, progressSec = sec, durationSec = dur,
        completed = completed, watchedAt = java.time.Instant.EPOCH, anilistId = 1, malId = null,
    )

    @Test
    fun `resume progress picks the first length longer than the watched time`() {
        assertNull(resumeProgress(null, null, 24))
        assertEquals(ResumeProgress(219, 1440), resumeProgress(hist(4, 219, 1440), null, null))
        assertEquals(1221, resumeProgress(hist(4, 219, 1440), null, null)?.remainingSec)
        // history duration not longer than the watched time: the episode runtime, then the title's length
        assertEquals(1380, resumeProgress(hist(4, 219, 100), Episode(4, null, null, null, runtimeMin = 23), 24)?.durationSec)
        assertEquals(1440, resumeProgress(hist(4, 219, null), Episode(4, null, null, null), 24)?.durationSec)
        val unknown = resumeProgress(hist(4, 219, null), null, 3)
        assertEquals(ResumeProgress(219, null), unknown)
        assertNull(unknown?.fraction)
        assertNull(unknown?.remainingSec)
    }

    @Test
    fun `watched count joins the list progress and completed history rows within the total`() {
        assertEquals(0, watchedCount(emptyMap(), 0, 12))
        assertEquals(3, watchedCount(emptyMap(), 3, 12))
        assertEquals(4, watchedCount(mapOf(7 to hist(7, 1400, 1400, completed = true), 2 to hist(2, 10, 1400)), 3, 12))
        assertEquals(2, watchedCount(mapOf(13 to hist(13, 1400, 1400, completed = true)), 2, 12))
        assertEquals(3, watchedCount(mapOf(13 to hist(13, 1400, 1400, completed = true)), 2, null))
    }

    @Test
    fun `head facts leave out what is missing`() {
        val m = media.copy(status = "RELEASING", year = 2026, score = 76, episodes = 12)
        val next = app.tatakai.next.model.MediaDetails(ref = m.ref, nextAiring = NextAiring(9, 1_000_000))
        val facts = headFacts(m, next, watched = 3, total = 12, nowSec = 1_000_000 - 2 * 86_400 - 5 * 3_600)
        assertEquals(
            listOf(HeadFact.Status("RELEASING"), HeadFact.Next(9, Countdown(2, 5, 0)), HeadFact.Episodes(3, 12), HeadFact.Year(2026), HeadFact.Score(76)),
            facts,
        )
        // aired already, unknown status, no year / score, nothing watched without a total
        assertEquals(emptyList(), headFacts(media.copy(status = "WEIRD", episodes = null), next, watched = 0, total = null, nowSec = 2_000_000))
        assertEquals(listOf(HeadFact.Episodes(3, null)), headFacts(media.copy(episodes = null), null, watched = 3, total = null, nowSec = 0))
        assertEquals(listOf(HeadFact.Episodes(0, 12)), headFacts(media, null, watched = 0, total = 12, nowSec = 0))
        // a movie has no episode fact
        assertEquals(emptyList(), headFacts(media.copy(episodes = 1), null, watched = 0, total = 1, nowSec = 0))
        assertEquals(emptyList(), headFacts(media.copy(format = "MOVIE"), null, watched = 0, total = 2, nowSec = 0))
    }

    @Test
    fun `head fit by window height and font scale`() {
        assertEquals(HeadFit.TIGHT, headFit(800, 1f))
        assertEquals(HeadFit.FULL, headFit(900, 1f))
        assertEquals(HeadFit.TIGHT, headFit(900, 1.3f))
        assertEquals(HeadFit.SHORT, headFit(650, 1f))
        // the borders
        assertEquals(HeadFit.SHORT, headFit(699, 1f))
        assertEquals(HeadFit.TIGHT, headFit(700, 1f))
        assertEquals(HeadFit.TIGHT, headFit(839, 1f))
        assertEquals(HeadFit.FULL, headFit(840, 1f))
        assertEquals(HeadFit.FULL, headFit(900, 1.14f))
        assertEquals(HeadFit.TIGHT, headFit(900, 1.15f))
    }

    @Test
    fun `grid rows and columns drop the score then the year`() {
        val five = listOf(HeadFact.Status("RELEASING"), HeadFact.Next(9, Countdown(2, 5, 0)), HeadFact.Episodes(3, 12), HeadFact.Year(2026), HeadFact.Score(76))
        assertEquals(five, gridFacts(five, 3, Int.MAX_VALUE))
        assertEquals(five.take(3), gridFacts(five, 3, 1))
        // 130 % font: 2 columns x 2 rows
        assertEquals(2, gridMaxRows(HeadFit.TIGHT, 1.3f))
        assertEquals(listOf(five[0], five[1], five[2], five[3]), gridFacts(five, 2, 2))
        val noNext = five.filterNot { it is HeadFact.Next }
        assertEquals(listOf(noNext[0], noNext[1], noNext[2]), gridFacts(noNext, 3, 1))
        assertEquals(1, gridMaxRows(HeadFit.SHORT, 1.3f))
        assertEquals(Int.MAX_VALUE, gridMaxRows(HeadFit.FULL, 1f))
        assertEquals(3, gridColumns(380f, 1f))
        assertEquals(2, gridColumns(380f, 1.3f))
        // a 600 dp window with the 340 dp card beside: ~228 dp for the grid -> 2 columns
        assertEquals(2, gridColumns(228f, 1f))
    }

    @Test
    fun `accessible name contains the visible label`() {
        assertEquals("Continuar episodio 4", labelInName("Continuar", "Continuar episodio 4"))
        assertEquals("Empezar episodio 1", labelInName("Empezar", "Empezar episodio 1"))
        // the abbreviated label is not inside the long form: the visible text is the name
        assertEquals("Continuar · ep. 4", labelInName("Continuar · ep. 4", "Continuar episodio 4"))
        assertEquals("Volver a ver desde el episodio 1", labelInName("Volver a ver", "Volver a ver desde el episodio 1"))
    }

    @Test
    fun `generic episode titles that repeat the number`() {
        assertEquals(true, isGenericEpisodeTitle("Episode 4", 4))
        assertEquals(true, isGenericEpisodeTitle("Episodio 04", 4))
        assertEquals(true, isGenericEpisodeTitle("Ep. 4", 4))
        assertEquals(false, isGenericEpisodeTitle("Episode 5", 4))
        assertEquals(false, isGenericEpisodeTitle("Item Crafting Face-Off", 4))
    }

    @Test
    fun `runtime lengths are estimates and minutes round`() {
        val own = resumeProgress(hist(4, 219, 1440), null, null)!!
        assertEquals(false, own.estimated)
        assertEquals(24, own.durationMin)
        val est = resumeProgress(hist(4, 219, null), Episode(4, null, null, null, runtimeMin = 24), null)!!
        assertEquals(true, est.estimated)
        assertEquals(20, est.remainingMin) // 1221 s
        assertEquals(1, roundedMinutes(30))
        assertEquals(0, roundedMinutes(29))
        assertEquals(24, roundedMinutes(1410))
    }

    @Test
    fun `first fitting label`() {
        assertEquals("b", firstFitting(listOf("aaaa", "b", "c")) { it.length < 2 })
        assertNull(firstFitting(listOf("aaaa", "bbb")) { it.length < 2 })
        // the Watch button's real candidates against a width in characters (~8.5 px each)
        val labels = listOf("Continuar episodio 123", "Continuar · ep. 123")
        val fits = { room: Int -> { s: String -> s.length * 8.5 <= room } }
        assertEquals("Continuar episodio 123", firstFitting(labels, fits(240)))
        assertEquals("Continuar · ep. 123", firstFitting(labels, fits(170)))
        assertNull(firstFitting(labels, fits(120)))
    }
}
