// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.FeedCache
import app.tatakai.next.database.FeedCacheDao
import app.tatakai.next.database.FeedCacheRow
import app.tatakai.next.metadata.ArtworkProvider
import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.MetadataRouter
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.model.MediaArtwork
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.RequestPriority
import app.tatakai.next.model.currentRequestPriority
import kotlinx.coroutines.runBlocking
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

// 0.3.2: ani.zip artwork is cached 7 days (also "none"), asked at background priority and never an error.
class MetadataArtworkTest {
    private class MemDao : FeedCacheDao {
        val rows = HashMap<String, FeedCacheRow>()
        override suspend fun get(key: String): FeedCacheRow? = rows[key]
        override suspend fun put(row: FeedCacheRow) { rows[row.key] = row }
        override suspend fun purgeOlderThan(olderThanMs: Long) { rows.values.removeAll { it.fetchedAtMs < olderThanMs } }
        override suspend fun clear() = rows.clear()
        override suspend fun count(): Int = rows.size
        override suspend fun totalBytes(): Long = 0
        override suspend fun containing(pattern: String, limit: Int): List<FeedCacheRow> = emptyList()
    }

    private class Zip : MetadataProvider, ArtworkProvider {
        override val id = "anizip"
        override val kinds = setOf(MediaKind.ANIME)
        override val capabilities = setOf(Capability.EPISODES)
        var calls = 0
        var art: MediaArtwork? = MediaArtwork(fanart = "f.jpg", clearlogo = "l.png")
        var fail = false
        val priorities = mutableListOf<RequestPriority>()
        override suspend fun artwork(ref: MediaRef): MediaArtwork? {
            calls++
            priorities += currentRequestPriority()
            if (fail) throw ProviderException(id, 0, "offline")
            return art
        }
    }

    private var now = 1_000_000_000L
    private fun repo(z: Zip) = MetadataRepositoryImpl(MetadataRouter(listOf(z), { listOf("anilist", "anizip") }, clock = { now }), FeedCache(MemDao()) { now })

    @Test
    fun cachedForAWeekAtBackgroundPriority() = runBlocking<Unit> {
        val z = Zip()
        val r = repo(z)
        val ref = MediaRef(anilistId = 212888)
        assertEquals("f.jpg", r.artwork(ref)?.fanart)
        now += 6L * 24 * 60 * 60_000
        assertEquals("l.png", r.artwork(ref)?.clearlogo)
        assertEquals(1, z.calls)
        assertEquals(listOf(RequestPriority.BACKGROUND), z.priorities)
        now += 2L * 24 * 60 * 60_000
        z.art = MediaArtwork(fanart = "g.jpg")
        assertEquals("g.jpg", r.artwork(ref)?.fanart)
        assertEquals(2, z.calls)
    }

    @Test
    fun noneIsCachedAndFailuresServeTheStaleCopyOrNull() = runBlocking<Unit> {
        val z = Zip().apply { art = null }
        val r = repo(z)
        val ref = MediaRef(anilistId = 1)
        assertNull(r.artwork(ref))
        assertNull(r.artwork(ref))
        assertEquals(1, z.calls)
        assertNull(r.artwork(MediaRef()))

        val z2 = Zip()
        val r2 = repo(z2)
        assertEquals("f.jpg", r2.artwork(ref)?.fanart)
        now += 8L * 24 * 60 * 60_000
        z2.fail = true
        assertEquals("f.jpg", r2.artwork(ref)?.fanart, "stale copy when ani.zip is down")
        assertNull(repo(Zip().apply { fail = true }).artwork(ref))
    }
}
