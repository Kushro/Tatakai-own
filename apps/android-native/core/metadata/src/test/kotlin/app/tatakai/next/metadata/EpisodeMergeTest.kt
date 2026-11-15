// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata

import app.tatakai.next.model.Episode
import app.tatakai.next.model.MediaArtwork
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertSame
import kotlin.test.assertTrue

/** 0.3.2: episode merge (Overgeared-like: Kitsu number-only + ani.zip rich) and ani.zip artwork. */
class EpisodeMergeTest {
    private fun fixture(name: String): JsonObject =
        Json.parseToJsonElement(requireNotNull(javaClass.getResource("/extra/$name")).readText()) as JsonObject

    private val now = Instant.parse("2026-09-12T00:00:00Z").toEpochMilli()
    private val kitsu by lazy { Mappers.kitsuEpisodes(fixture("kitsu-episodes-overgeared.json")) }
    private val zip by lazy { Mappers.aniZipEpisodes(fixture("anizip-overgeared.json")) }

    @Test
    fun `kitsu number-only base is filled by number from ani zip`() {
        assertEquals(8, kitsu.size)
        assertTrue(kitsu.all { it.image == null && it.summary == null })
        assertTrue(EpisodeMerge.hasGaps(kitsu, now))
        val merged = EpisodeMerge.merge(kitsu, zip, now)
        // 1..8 from Kitsu, 9 and 10 aired by now (ani.zip only), 11 / 12 are future: not added
        assertEquals((1..10).toList(), merged.map { it.number })
        merged.take(8).forEachIndexed { i, e ->
            val n = i + 1
            assertEquals("Grid Part $n", e.title)
            assertEquals("https://artworks.thetvdb.com/banners/v4/episode/${1000 + n}/screencap/a.jpg", e.image)
            assertEquals("Shin Youngwoo, episode $n.", e.summary)
            assertEquals(24, e.runtimeMin)
            assertTrue(e.rating != null)
            // the base's own air date is kept
            assertEquals(kitsu[i].airDate, e.airDate)
        }
        assertNull(merged[8].image)
        assertEquals("Grid Part 9", merged[8].title)
        // distinct thumbnails: no more "every episode shows the series art"
        assertEquals(8, merged.mapNotNull { it.image }.distinct().size)
        // 9 / 10 have no image yet: still a gap, so the router may ask the next provider
        assertTrue(EpisodeMerge.hasGaps(merged, now))
    }

    @Test
    fun `base fields win and generic titles count as missing`() {
        val base = listOf(
            Episode(1, "Real title", "b1.jpg", "2026-07-01", summary = "base summary"),
            Episode(2, "Episodio 2", null, null),
            Episode(3, "第3話", null, null),
        )
        val extra = listOf(
            Episode(1, "Other", "x1.jpg", "2026-07-02", summary = "x", runtimeMin = 23, rating = 8.1),
            Episode(2, "Two", "x2.jpg", "2026-07-09"),
            Episode(3, "Episode 3", null, null),
            Episode(4, "Four", null, null),
            Episode(5, "Five", null, null),
        )
        val m = EpisodeMerge.merge(base, extra, now)
        assertEquals(Episode(1, "Real title", "b1.jpg", "2026-07-01", summary = "base summary", runtimeMin = 23, rating = 8.1), m[0])
        assertEquals(Episode(2, "Two", "x2.jpg", "2026-07-09"), m[1])
        assertEquals("第3話", m[2].title, "nothing better: the base's placeholder stays")
        // undated numbers past the base's last one are not added (could be unaired)
        assertEquals(listOf(1, 2, 3), m.map { it.number })
        assertSame(base, EpisodeMerge.merge(base, emptyList(), now))
        assertTrue(EpisodeMerge.isGenericTitle("Ep. 12"))
        assertTrue(EpisodeMerge.isGenericTitle("  "))
        assertFalse(EpisodeMerge.isGenericTitle("Episode 3: The Return"))
    }

    @Test
    fun `future episodes are not gaps`() {
        val list = listOf(Episode(1, "One", "1.jpg", "2026-07-01", summary = "s"), Episode(2, null, null, "2027-01-01T00:00:00Z"))
        assertFalse(EpisodeMerge.hasGaps(list, now))
        assertEquals(Instant.parse("2026-07-01T00:00:00Z").toEpochMilli(), EpisodeMerge.airedAtMs("2026-07-01"))
        assertNull(EpisodeMerge.airedAtMs("soon"))
    }

    @Test
    fun `ani zip artwork maps every cover type, first one wins`() {
        val art = Mappers.aniZipArtwork(fixture("anizip-overgeared.json"))
        assertEquals(
            MediaArtwork(
                fanart = "https://artworks.thetvdb.com/banners/v4/series/1/backgrounds/f.jpg",
                banner = "https://artworks.thetvdb.com/banners/v4/series/1/banners/b.jpg",
                poster = "https://artworks.thetvdb.com/banners/v4/series/1/posters/p.jpg",
                clearlogo = "https://artworks.thetvdb.com/banners/v4/series/1/clearlogo/l.png",
            ),
            art,
        )
        val frieren = Mappers.aniZipArtwork(Fixtures.element("anizip-frieren.json") as JsonObject)
        assertEquals("https://artworks.thetvdb.com/banners/v4/series/424536/backgrounds/64e6cbe29d9c0.jpg", frieren?.fanart)
        assertEquals("https://artworks.thetvdb.com/banners/v4/series/424536/clearlogo/696a802a5aa22.png", frieren?.clearlogo)
        assertNull(Mappers.aniZipArtwork(JsonObject(emptyMap())))
    }

    private class EpFake(override val id: String, val list: () -> List<Episode>) : MetadataProvider, ArtworkProvider {
        override val kinds = setOf(MediaKind.ANIME)
        override val capabilities = setOf(Capability.EPISODES)
        var calls = 0
        var art: MediaArtwork? = null
        override suspend fun episodes(ref: MediaRef): List<Episode> { calls++; return list() }
        override suspend fun artwork(ref: MediaRef): MediaArtwork? = art
    }

    @Test
    fun `router merges the first list with ani zip even when kitsu comes first`() = runBlocking<Unit> {
        val k = EpFake("kitsu") { kitsu }
        val z = EpFake("anizip") { zip }
        val t = EpFake("tenrai") { emptyList() }
        val r = MetadataRouter(listOf(k, z, t), { listOf("anilist", "kitsu", "tenrai", "anizip") }, clock = { now })
        val routed = r.episodes(MediaRef(anilistId = 212888))
        assertEquals("kitsu", routed.provider)
        assertEquals((1..10).toList(), routed.value.map { it.number })
        assertEquals("Grid Part 1", routed.value[0].title)
        assertEquals(1, z.calls, "ani.zip is asked first among the fillers")
        assertEquals(1, t.calls, "9 / 10 still lack a thumbnail: the next filler is asked")
    }

    @Test
    fun `router does not ask fillers for a complete list, fillers failing never fail`() = runBlocking<Unit> {
        val z = EpFake("anizip") { zip }
        val k = EpFake("kitsu") { throw ProviderException("kitsu", 500, "boom") }
        val r = MetadataRouter(listOf(k, z), { listOf("anizip", "kitsu") }, clock = { Instant.parse("2026-08-20T00:00:00Z").toEpochMilli() })
        val routed = r.episodes(MediaRef(anilistId = 212888))
        assertEquals("anizip", routed.provider)
        // ani.zip's list up to 8 is complete by Aug 20 (later ones are future): Kitsu not asked
        assertEquals(0, k.calls)
        assertEquals(12, routed.value.size)

        val k2 = EpFake("kitsu") { kitsu }
        val z2 = EpFake("anizip") { throw ProviderException("anizip", 503, "down") }
        val r2 = MetadataRouter(listOf(k2, z2), { listOf("kitsu", "anizip") }, clock = { now })
        assertEquals(kitsu, r2.episodes(MediaRef(anilistId = 212888)).value)
    }

    @Test
    fun `artwork comes from the enabled artwork provider`() = runBlocking<Unit> {
        val z = EpFake("anizip") { emptyList() }.apply { art = MediaArtwork(fanart = "f") }
        assertEquals("f", MetadataRouter(listOf(z), { listOf("anizip") }).artwork(MediaRef(anilistId = 1))?.fanart)
        assertNull(MetadataRouter(listOf(z), { listOf("kitsu") }).artwork(MediaRef(anilistId = 1)), "disabled provider")
    }
}
