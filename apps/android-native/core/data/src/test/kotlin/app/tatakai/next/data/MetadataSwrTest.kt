// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.FeedCache
import app.tatakai.next.database.FeedCacheDao
import app.tatakai.next.database.FeedCacheRow
import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.MetadataRouter
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.RequestPriority
import app.tatakai.next.model.currentRequestPriority
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.cancel
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlinx.coroutines.yield
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals

// 0.3.1: Home rows are stale-while-revalidate; concurrent lookups of one title share one router call.
class MetadataSwrTest {
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

    private fun media(id: Int, title: String) = Media(
        ref = MediaRef(anilistId = id), kind = MediaKind.ANIME, title = title, titles = emptyMap(), cover = null, banner = null,
        color = null, synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null,
        sourceProvider = "anilist",
    )

    private class Fake : MetadataProvider {
        override val id = "anilist"
        override val kinds = setOf(MediaKind.ANIME)
        override val capabilities = setOf(Capability.FEED, Capability.MEDIA)
        val feedCalls = AtomicInteger()
        val mediaCalls = AtomicInteger()
        var title = "v1"
        val priorities = mutableListOf<RequestPriority>()
        var gate: CompletableDeferred<Unit>? = null
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
            feedCalls.incrementAndGet()
            val priority = currentRequestPriority()
            synchronized(priorities) { priorities += priority }
            gate?.await()
            return FeedPage(listOf(Media(MediaRef(anilistId = 1), MediaKind.ANIME, title, emptyMap(), null, null, null, null, emptyList(), null, null, null, null, null, "anilist")), page, false)
        }
        override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
            mediaCalls.incrementAndGet()
            gate?.await()
            return Media(ref, MediaKind.ANIME, "Frieren", emptyMap(), "c", null, null, "s", listOf("Drama"), 28, null, null, 2023, 90, "anilist")
        }
    }

    private var now = 1_000_000_000L
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    @AfterTest
    fun tearDown() = scope.cancel()

    private fun repo(p: Fake, swr: Boolean = true) =
        MetadataRepositoryImpl(MetadataRouter(listOf(p), { listOf("anilist") }), FeedCache(MemDao()) { now }, refreshScope = if (swr) scope else null)

    @Test
    fun freshRowsAreServedWithoutCalls() = runBlocking<Unit> {
        val p = Fake()
        val r = repo(p)
        r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1)
        now += 14 * 60_000
        r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1)
        assertEquals(1, p.feedCalls.get())
        // Home rows are background traffic for the AniList gateway
        assertEquals(listOf(RequestPriority.BACKGROUND), p.priorities)
    }

    @Test
    fun staleRowsAreShownAtOnceAndRefreshedInTheBackground() = runBlocking<Unit> {
        val p = Fake()
        val r = repo(p)
        r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1)
        now += 16 * 60_000
        p.title = "v2"
        assertEquals("v1", r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1).items.single().title)
        withTimeout(5_000) { while (p.feedCalls.get() < 2) yield() }
        // the refresh landed: the next read is fresh v2 without another call
        withTimeout(5_000) {
            while (r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1).items.single().title != "v2") yield()
        }
        assertEquals(2, p.feedCalls.get())
    }

    @Test
    fun tooOldRowsAreFetchedFirst() = runBlocking<Unit> {
        val p = Fake()
        val r = repo(p)
        r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1)
        now += 25 * 60 * 60_000L
        p.title = "v2"
        assertEquals("v2", r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1).items.single().title)
    }

    @Test
    fun withoutAScopeStaleRowsAreFetchedFirst() = runBlocking<Unit> {
        val p = Fake()
        val r = repo(p, swr = false)
        r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1)
        now += 16 * 60_000
        p.title = "v2"
        assertEquals("v2", r.feed(MediaKind.ANIME, FeedKind.POPULAR, 1).items.single().title)
    }

    @Test
    fun concurrentLookupsOfOneTitleShareOneCall() = runBlocking<Unit> {
        val p = Fake().apply { gate = CompletableDeferred() }
        val r = repo(p)
        val ref = MediaRef(anilistId = 154587)
        val all = (1..3).map { async(Dispatchers.Default) { r.media(MediaKind.ANIME, ref) } }
        withTimeout(5_000) { while (p.mediaCalls.get() < 1) yield() }
        p.gate!!.complete(Unit)
        val results = all.awaitAll()
        assertEquals(1, p.mediaCalls.get())
        assertEquals(setOf("Frieren"), results.map { it?.title }.toSet())
        // and the next one is a cache hit
        r.media(MediaKind.ANIME, ref)
        assertEquals(1, p.mediaCalls.get())
    }
}
