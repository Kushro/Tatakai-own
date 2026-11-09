// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.Container
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import java.time.LocalDate
import java.time.ZoneOffset
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class HomeLogicTest {
    private fun media(id: Int, genres: List<String> = emptyList()) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = "A$id", titles = emptyMap(), cover = null, banner = null,
        color = null, synopsis = null, genres = genres, episodes = null, status = "RELEASING", season = null, year = null, score = null,
        sourceProvider = "anilist",
    )

    @Test fun latestEpisodesKeepsAiredNewestOnePerSeries() {
        val now = 10_000L
        val list = listOf(
            AiringEpisode(media(1), 3, 9_000),
            AiringEpisode(media(1), 4, 9_500),
            AiringEpisode(media(2), 1, 8_000),
            AiringEpisode(media(3), 7, 12_000), // future
        )
        val out = HomeLogic.latestEpisodes(list, now)
        assertEquals(listOf(1 to 4, 2 to 1), out.map { it.media.ref.anilistId to it.episode })
    }

    @Test fun parsesProviderHistoryIds() {
        assertEquals(Triple("ext1", "src", "item9"), HomeLogic.parseProviderHistoryId("ext:ext1:src:item9"))
        assertEquals(Triple("server:pc/ext", "src", "i"), HomeLogic.parseProviderHistoryId("ext:server:pc/ext:src:i"))
        assertNull(HomeLogic.parseProviderHistoryId("anilist:21"))
        assertNull(HomeLogic.parseProviderHistoryId("ext:only"))
    }

    @Test fun matureDetectionUsesHentaiGenre() {
        assertTrue(HomeLogic.isMature(media(1, listOf("hentai"))))
        assertFalse(HomeLogic.isMature(media(1, listOf("Action"))))
    }

    @Test fun extensionRowShowsNameWhenKnown() {
        val row = ExtensionHomeRow("e1", "s1", "Popular", emptyList())
        assertEquals("My Ext", HomeLogic.extensionRowSubtitle(row, mapOf("e1" to "My Ext")))
        assertEquals("e1", HomeLogic.extensionRowSubtitle(row, emptyMap()))
    }

    @Test fun activityCountsOnlyRunningWebDownloads() {
        fun item(id: String, st: DownloadState) = DownloadItem(
            id = id, ref = MediaRef(anilistId = 1), title = "t", episode = 1, extId = "e", sourceLabel = "s",
            container = Container.HLS, state = st, createdAtMs = 0,
        )
        val a = HomeLogic.activity(2, listOf(item("a", DownloadState.DOWNLOADING), item("b", DownloadState.COMPLETED), item("c", DownloadState.QUEUED)), emptyList())
        assertEquals(2, a.webActive)
        assertEquals(4, a.total)
    }

    @Test fun calendarGroupsIntoSevenLocalDays() {
        val monday = LocalDate.of(2026, 11, 9)
        val zone = ZoneOffset.UTC
        val (from, to) = CalendarLogic.weekWindow(monday, zone)
        assertEquals(7 * 86_400L, to - from)
        val eps = listOf(AiringEpisode(media(1), 1, from + 3_600), AiringEpisode(media(2), 2, from + 86_400 * 2 + 10))
        val days = CalendarLogic.group(eps, monday, zone)
        assertEquals(7, days.size)
        assertEquals(1, days[0].episodes.size)
        assertEquals(1, days[2].episodes.size)
        assertEquals(monday, CalendarLogic.mondayOf(LocalDate.of(2026, 11, 15)))
    }
}
