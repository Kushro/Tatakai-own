// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.CalendarView
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaArtwork
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import java.time.Instant
import java.time.LocalDate
import java.time.YearMonth
import java.time.ZoneOffset
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** 2026-11 UI refresh: spotlight order / continue lookup and the calendar views' pure logic. */
class LookHomeLogicTest {
    private val utc = ZoneOffset.UTC

    private fun media(id: Int, banner: String? = null) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "A$id", titles = emptyMap(), cover = "c$id", banner = banner,
        color = null, synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null,
        sourceProvider = "anilist",
    )

    @Test
    fun spotlightPutsTitlesWithoutBannerLast() {
        val list = listOf(media(1), media(2, "b2"), media(3, ""), media(4, "b4"))
        assertEquals(listOf(2, 4, 1, 3), spotlightOrder(list).map { it.ref.anilistId })
    }

    @Test
    fun spotlightPrefersFanartThenBannerThenCover() {
        val m = media(1, "banner.jpg").copy(cover = "cover.jpg")
        val full = MediaArtwork(fanart = "fanart.jpg", banner = "tvdb-banner.jpg", clearlogo = "logo.png")
        assertEquals(SpotlightArt("fanart.jpg", lowRes = false, fromBanner = false, logo = "logo.png"), spotlightArt(m, full))
        // no fanart: AniList's 1900x400 banner, flagged low-res (cropped band + stronger scrim), the logo still shown
        assertEquals(SpotlightArt("banner.jpg", lowRes = true, fromBanner = true, logo = "logo.png"), spotlightArt(m, full.copy(fanart = null)))
        assertEquals(SpotlightArt("banner.jpg", lowRes = true, fromBanner = true, logo = null), spotlightArt(m, null))
        assertEquals(SpotlightArt("cover.jpg", lowRes = true, fromBanner = false, logo = null), spotlightArt(m.copy(banner = " "), MediaArtwork(fanart = "")))
        assertEquals(null, spotlightArt(m.copy(banner = null, cover = null), null).backdrop)
    }

    @Test
    fun continueEntryMatchesByAniListOrMal() {
        val e = HistoryEntry("5", "x", null, "5?ep=3", 3, 10, 100, false, Instant.EPOCH, anilistId = 5, malId = 50)
        assertEquals(e, continueEntryFor(media(5), listOf(e)))
        assertEquals(e, continueEntryFor(media(9).copy(ref = MediaRef(malId = 50)), listOf(e)))
        assertNull(continueEntryFor(media(6), listOf(e)))
    }

    @Test
    fun monthGridCoversWholeWeeks() {
        // February 2027 starts on a Monday and has 28 days: exactly four weeks.
        val feb = CalendarLogic.monthGrid(YearMonth.of(2027, 2))
        assertEquals(28, feb.size)
        assertEquals(LocalDate.of(2027, 2, 1), feb.first())
        // November 2026 starts on a Sunday: the grid starts on Monday Oct 26 and ends on Sunday Dec 6 (6 weeks).
        val nov = CalendarLogic.monthGrid(YearMonth.of(2026, 11))
        assertEquals(42, nov.size)
        assertEquals(LocalDate.of(2026, 10, 26), nov.first())
        assertEquals(LocalDate.of(2026, 12, 6), nov.last())
        assertEquals(java.time.DayOfWeek.MONDAY, nov.first().dayOfWeek)
    }

    @Test
    fun rangesAndPagesPerView() {
        val wed = LocalDate.of(2026, 11, 11)
        assertEquals(LocalDate.of(2026, 11, 9) to 7, CalendarLogic.range(CalendarView.WEEK, wed))
        assertEquals(LocalDate.of(2026, 11, 9) to 7, CalendarLogic.range(CalendarView.AGENDA, wed))
        assertEquals(wed to 1, CalendarLogic.range(CalendarView.DAY, wed))
        assertEquals(LocalDate.of(2026, 10, 26) to 42, CalendarLogic.range(CalendarView.MONTH, wed))
        assertEquals(LocalDate.of(2026, 11, 18), CalendarLogic.shift(CalendarView.WEEK, wed, 1))
        assertEquals(LocalDate.of(2026, 10, 1), CalendarLogic.shift(CalendarView.MONTH, LocalDate.of(2026, 11, 30), -1))
        assertEquals(LocalDate.of(2026, 11, 10), CalendarLogic.shift(CalendarView.DAY, wed, -1))
    }

    @Test
    fun dayLinesGroupByHourAndPlaceNow() {
        val day = LocalDate.of(2026, 11, 11)
        fun at(h: Int, m: Int = 0) = day.atTime(h, m).toEpochSecond(utc)
        val a = AiringEpisode(media(1), 1, at(9))
        val b = AiringEpisode(media(2), 2, at(9, 30))
        val c = AiringEpisode(media(3), 3, at(22))
        val lines = CalendarLogic.dayLines(listOf(c, a, b), day, utc, nowSec = at(12))
        assertEquals(
            listOf(
                CalendarLogic.DayLine.Hour(9), CalendarLogic.DayLine.Ep(a), CalendarLogic.DayLine.Ep(b),
                CalendarLogic.DayLine.Now,
                CalendarLogic.DayLine.Hour(22), CalendarLogic.DayLine.Ep(c),
            ),
            lines,
        )
        // Another day: no "now" marker; after everything aired: the marker goes last.
        assertEquals(false, CalendarLogic.dayLines(listOf(a), day, utc, nowSec = at(12) + 86_400).contains(CalendarLogic.DayLine.Now))
        assertEquals(CalendarLogic.DayLine.Now, CalendarLogic.dayLines(listOf(a), day, utc, nowSec = at(23)).last())
    }
}
