// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.FeedCache
import app.tatakai.next.database.FeedCacheDao
import app.tatakai.next.database.FeedCacheRow
import app.tatakai.next.database.HistoryDao
import app.tatakai.next.database.HistoryRow
import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.MetadataRouter
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaDetails
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.searchTitles
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

// BUG-143: with AniList rate limited, Kitsu's answer ("Temppal: Item no Chikara", no English title) was cached fresh for 24 h and
// was the only thing the extensions got, so Toko found no torrent of Overgeared for a day.
class MetadataFallbackTest {
    private class MemDao : FeedCacheDao {
        val rows = HashMap<String, FeedCacheRow>()
        override suspend fun get(key: String): FeedCacheRow? = rows[key]
        override suspend fun put(row: FeedCacheRow) { rows[row.key] = row }
        override suspend fun purgeOlderThan(olderThanMs: Long) { rows.values.removeAll { it.fetchedAtMs < olderThanMs } }
        override suspend fun clear() = rows.clear()
        override suspend fun count(): Int = rows.size
        override suspend fun totalBytes(): Long = rows.values.sumOf { it.json.length.toLong() }
    }

    private class Fake(override val id: String, var answer: () -> Media?) : MetadataProvider {
        override val kinds = setOf(MediaKind.ANIME)
        override val capabilities = setOf(Capability.MEDIA)
        var calls = 0
        override suspend fun media(kind: MediaKind, ref: MediaRef): Media? { calls++; return answer() }
    }

    private val ref = MediaRef(anilistId = 212888)
    private val key = "media:ANIME:212888:null:null"

    private val anilistOg = Media(
        ref = MediaRef(212888, null, null), kind = MediaKind.ANIME, title = "Overgeared",
        titles = linkedMapOf("romaji" to "Tempal: Item no Chikara", "english" to "Overgeared", "native" to "テムパル～アイテムの力～"),
        cover = "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/bx212888.jpg", banner = null, color = null,
        synopsis = "Grid", genres = listOf("Action"), episodes = 12, status = "RELEASING", season = "SUMMER", year = 2026, score = 76,
        sourceProvider = "anilist",
    )
    private val kitsuOg = Media(
        ref = MediaRef(212888, null, 50743), kind = MediaKind.ANIME, title = "Temppal: Item no Chikara",
        titles = linkedMapOf("romaji" to "Temppal: Item no Chikara", "native" to "テムパル～アイテムの力～"),
        cover = "https://media.kitsu.app/anime/50743/poster_image/large-9b11.jpeg", banner = null, color = null,
        synopsis = "Grid", genres = listOf("Action"), episodes = 12, status = "RELEASING", season = "SUMMER", year = 2026, score = 76,
        sourceProvider = "kitsu",
    )
    private val rateLimited: () -> Media? = { throw ProviderException("anilist", 429, "HTTP 429") }

    private var now = 1_000_000_000L
    private val dao = MemDao()
    private val anilist = Fake("anilist") { anilistOg }
    private val kitsu = Fake("kitsu") { kitsuOg }
    private val router = MetadataRouter(listOf(anilist, kitsu), { listOf("anilist", "kitsu") }, clock = { now })
    private val repo = MetadataRepositoryImpl(router, FeedCache(dao) { now })

    private val hour = 60 * 60_000L
    private val minute = 60_000L

    @Test
    fun fallbackNeverReplacesACompleteEntryAndKeepsItsTitlesFirst() = runBlocking<Unit> {
        assertEquals("Overgeared", repo.media(MediaKind.ANIME, ref)?.title)
        val complete = dao.rows.getValue(key)

        now += 25 * hour // the complete row is stale; AniList is rate limited
        anilist.answer = rateLimited
        val m = repo.media(MediaKind.ANIME, ref)!!
        assertEquals("Overgeared", m.title)
        assertEquals(anilistOg.cover, m.cover, "the AniList cover stays (history, Details)")
        assertEquals(
            listOf("Overgeared", "Tempal: Item no Chikara", "テムパル～アイテムの力～", "Temppal: Item no Chikara"),
            m.searchTitles(),
            "AniList's order first, Kitsu's spelling appended",
        )
        assertEquals(complete, dao.rows.getValue(key), "the complete row is not overwritten (nor made fresh)")

        // next open after the cool-down: AniList answers again, the row is complete and fresh, no alt titles
        now += 2 * minute
        anilist.answer = { anilistOg }
        val back = repo.media(MediaKind.ANIME, ref)!!
        assertEquals(listOf("Overgeared", "Tempal: Item no Chikara", "テムパル～アイテムの力～"), back.searchTitles())
        assertEquals(now, dao.rows.getValue(key).fetchedAtMs)
    }

    @Test
    fun fallbackWithoutACompleteEntryIsCachedFifteenMinutesOnly() = runBlocking<Unit> {
        anilist.answer = rateLimited
        assertEquals("Temppal: Item no Chikara", repo.media(MediaKind.ANIME, ref)?.title)
        assertEquals(1, kitsu.calls)
        assertTrue(dao.rows.getValue(key).json.contains("\"degraded\":true"))

        now += 10 * minute
        repo.media(MediaKind.ANIME, ref)
        assertEquals(1, kitsu.calls, "served from the cache within 15 min")
        assertEquals(1, anilist.calls)

        now += 6 * minute // 16 min: retried; AniList is back
        anilist.answer = { anilistOg }
        val m = repo.media(MediaKind.ANIME, ref)!!
        assertEquals(2, anilist.calls)
        assertEquals("Overgeared", m.title)
        assertFalse(dao.rows.getValue(key).json.contains("\"degraded\":true"))

        now += 23 * hour
        repo.media(MediaKind.ANIME, ref)
        assertEquals(2, anilist.calls, "a complete row keeps the normal 24 h TTL")
    }

    @Test
    fun fallbackKeepsTitlesKnownBeforeAndAniListSynonyms() = runBlocking<Unit> {
        val json = Json { encodeDefaults = true }
        dao.rows["details:212888:null:null"] = FeedCacheRow(
            "details:212888:null:null",
            json.encodeToString(MediaDetails.serializer(), MediaDetails(ref = ref, synonyms = listOf("Overgeared", "Temple"))),
            now - 30 * hour,
        )
        anilist.answer = rateLimited
        val m = repo.media(MediaKind.ANIME, ref)!!
        assertEquals(
            listOf("Temppal: Item no Chikara", "テムパル～アイテムの力～", "Overgeared", "Temple"),
            m.searchTitles(),
        )
    }

    @Test
    fun aBareKitsuRowFromBeforeTheFixIsRetried() = runBlocking<Unit> {
        // the row r12 left on the phone: a bare Media from Kitsu, "fresh" for 24 h
        val json = Json { encodeDefaults = true }
        dao.rows[key] = FeedCacheRow(key, json.encodeToString(Media.serializer(), kitsuOg), now - hour)
        assertEquals("Overgeared", repo.media(MediaKind.ANIME, ref)?.title)
        assertEquals(1, anilist.calls)
    }

    @Test
    fun aFallbackPosterDoesNotReplaceTheAniListOneInTheHistory() = runBlocking<Unit> {
        val rows = HashMap<String, HistoryRow>()
        val historyDao = object : HistoryDao() {
            override fun observeRecent(limit: Int): Flow<List<HistoryRow>> = flowOf(rows.values.toList())
            override fun observeForAnime(anilistId: Int, animeId: String): Flow<List<HistoryRow>> =
                flowOf(rows.values.filter { it.animeId == animeId })
            override suspend fun get(episodeId: String): HistoryRow? = rows[episodeId]
            override suspend fun dirty(limit: Int): List<HistoryRow> = emptyList()
            override suspend fun dirtyCount(): Int = 0
            override suspend fun upsert(row: HistoryRow) { rows[row.episodeId] = row }
            override suspend fun markSynced(episodeId: String, watchedAtMs: Long) {}
            override suspend fun clear() = rows.clear()
            override suspend fun all(): List<HistoryRow> = rows.values.toList()
        }
        val history = HistoryRepositoryImpl(historyDao, object : HistorySyncScheduler {
            override fun requestSync(immediate: Boolean) {}
        })
        fun entry(ep: Int, poster: String) = HistoryEntry(
            animeId = "212888", title = "Overgeared", poster = poster, episodeId = "212888?ep=$ep", episode = ep, progressSec = 60,
            durationSec = 1440, completed = false, watchedAt = Instant.ofEpochMilli(now + ep), anilistId = 212888, malId = null,
        )
        history.record(entry(3, anilistOg.cover!!))
        history.record(entry(4, kitsuOg.cover!!))
        assertEquals(anilistOg.cover, rows.getValue("212888?ep=4").poster, "the series' AniList poster is kept")
        history.record(entry(4, "https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/new.jpg"))
        assertEquals("https://s4.anilist.co/file/anilistcdn/media/anime/cover/large/new.jpg", rows.getValue("212888?ep=4").poster)

        rows.clear()
        history.record(entry(1, kitsuOg.cover!!))
        assertEquals(kitsuOg.cover, rows.getValue("212888?ep=1").poster, "nothing better known: the fallback poster is used")
    }
}
