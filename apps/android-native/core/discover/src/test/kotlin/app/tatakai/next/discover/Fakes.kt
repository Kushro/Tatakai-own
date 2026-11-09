// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository

internal fun media(id: Int, genres: List<String> = listOf("Action"), year: Int? = 2020, score: Int? = 75, title: String = "T$id") = Media(
    ref = MediaRef(anilistId = id, malId = id * 10), kind = MediaKind.ANIME, title = title, titles = emptyMap(), cover = null, banner = null,
    color = null, synopsis = null, genres = genres, episodes = 12, status = "RELEASING", season = null, year = year, score = score,
    sourceProvider = "anilist",
)

internal class FakeMetadata(var schedule: List<AiringEpisode> = emptyList()) : MetadataRepository {
    var scheduleCalls = 0
    override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int) = FeedPage(emptyList(), page, false)
    override suspend fun search(kind: MediaKind, query: String, page: Int) = FeedPage(emptyList(), page, false)
    override suspend fun media(kind: MediaKind, ref: MediaRef): Media? = null
    override suspend fun episodes(ref: MediaRef): List<Episode> = emptyList()
    override suspend fun airingSchedule(fromEpochSec: Long, toEpochSec: Long, page: Int): List<AiringEpisode> {
        scheduleCalls++
        return if (page == 1) schedule.filter { it.airingAtEpochSec in fromEpochSec until toEpochSec } else emptyList()
    }
}
