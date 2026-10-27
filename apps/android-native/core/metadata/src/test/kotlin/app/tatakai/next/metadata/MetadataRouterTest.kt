package app.tatakai.next.metadata

import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.runBlocking
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

class MetadataRouterTest {
    private fun media(id: Int, provider: String) = Media(
        MediaRef(anilistId = id), MediaKind.ANIME, "T$id", emptyMap(), null, null, null, null,
        emptyList(), null, null, null, null, null, provider,
    )

    private class Fake(
        override val id: String,
        var behaviour: () -> FeedPage,
        override val capabilities: Set<Capability> = setOf(Capability.FEED, Capability.SEARCH),
    ) : MetadataProvider {
        override val kinds = setOf(MediaKind.ANIME)
        var calls = 0
        override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage { calls++; return behaviour() }
        override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage { calls++; return behaviour() }
    }

    private fun page(vararg m: Media) = FeedPage(m.toList(), 1, false)

    @Test
    fun usesPriorityOrder() = runBlocking<Unit> {
        val a = Fake("a", { page(media(1, "a")) })
        val b = Fake("b", { page(media(2, "b")) })
        val r = MetadataRouter(listOf(a, b), { listOf("b", "a") })
        assertEquals("b", r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1).provider)
        assertEquals(0, a.calls)
    }

    @Test
    fun skipsProvidersNotInPriorityAndUnsupportedCapabilities() = runBlocking<Unit> {
        val a = Fake("a", { page(media(1, "a")) })
        val noFeed = Fake("c", { page(media(3, "c")) }, capabilities = setOf(Capability.EPISODES))
        val r = MetadataRouter(listOf(noFeed, a), { listOf("c") })
        assertFailsWith<NoMetadataProviderException> { r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1) }
    }

    @Test
    fun fallsBackOnErrorAndCoolsDownFor60Seconds() = runBlocking<Unit> {
        var now = 1_000L
        val a = Fake("a", { throw ProviderException("a", 429, "HTTP 429") })
        val b = Fake("b", { page(media(2, "b")) })
        val r = MetadataRouter(listOf(a, b), { listOf("a", "b") }, clock = { now })

        assertEquals("b", r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1).provider)
        assertEquals(1, a.calls)
        assertTrue(r.isCoolingDown("a"))

        now += 59_000
        r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1)
        assertEquals(1, a.calls, "still cooling down at +59 s")

        now += 2_000 // 61 s after the failure
        a.behaviour = { page(media(1, "a")) }
        assertEquals("a", r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1).provider)
        assertEquals(2, a.calls)
        assertTrue(!r.isCoolingDown("a"))
    }

    @Test
    fun emptyResultFallsThroughButIsReturnedWhenNobodyHasData() = runBlocking<Unit> {
        val a = Fake("a", { page() })
        val b = Fake("b", { page() })
        val r = MetadataRouter(listOf(a, b), { listOf("a", "b") })
        val res = r.search(MediaKind.ANIME, "zzz", 1)
        assertEquals("a", res.provider)
        assertTrue(res.value.items.isEmpty())
        assertEquals(1, b.calls)
    }

    @Test
    fun malformedPayloadDoesNotTripCooldown() = runBlocking<Unit> {
        val a = Fake("a", { throw IllegalStateException("bad mapper input") })
        val b = Fake("b", { page(media(2, "b")) })
        val r = MetadataRouter(listOf(a, b), { listOf("a", "b") })
        r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1)
        assertTrue(!r.isCoolingDown("a"))
    }

    @Test
    fun retriesCoolingProvidersWhenEverythingIsDown() = runBlocking<Unit> {
        val a = Fake("a", { throw ProviderException("a", 0, "offline") })
        val r = MetadataRouter(listOf(a), { listOf("a") })
        assertFailsWith<NoMetadataProviderException> { r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1) }
        a.behaviour = { page(media(1, "a")) }
        assertEquals("a", r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1).provider)
    }

    @Test
    fun reportsRateLimitedProvider() = runBlocking<Unit> {
        val a = Fake("a", { throw ProviderException("a", 429, "HTTP 429") })
        val r = MetadataRouter(listOf(a), { listOf("a") })
        val e = assertFailsWith<NoMetadataProviderException> { r.feed(MediaKind.ANIME, FeedKind.TRENDING, 1) }
        assertEquals("a", e.rateLimitedProvider)
    }
}
