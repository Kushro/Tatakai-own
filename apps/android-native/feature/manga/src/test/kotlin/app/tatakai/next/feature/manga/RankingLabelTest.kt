// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import app.tatakai.next.manga.MangaRanking
import org.junit.Test
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull

/** BUG-111: AniList rankings are worded by the app, not shown as AniList's English context. */
class RankingLabelTest {
    @Test
    fun ratedAndPopularAllTimeOrOfAYear() {
        assertEquals(R.string.manga_ranking_rated_all_time, rankingLabelRes(MangaRanking(97, "RATED", true, null, "highest rated all time")))
        assertEquals(R.string.manga_ranking_popular_all_time, rankingLabelRes(MangaRanking(1, "POPULAR", true, null, "most popular all time")))
        assertEquals(R.string.manga_ranking_rated_year, rankingLabelRes(MangaRanking(6, "RATED", false, 2018, "highest rated")))
        assertEquals(R.string.manga_ranking_popular_year, rankingLabelRes(MangaRanking(1, "POPULAR", false, 2018, "most popular")))
        assertNull(rankingLabelRes(MangaRanking(3, "TRENDING", true, null, "trending")))
        assertNull(rankingLabelRes(MangaRanking(3, "RATED", false, null, "highest rated")))
    }
}
