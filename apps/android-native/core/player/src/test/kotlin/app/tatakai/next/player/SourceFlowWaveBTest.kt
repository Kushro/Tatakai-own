// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.Container
import app.tatakai.next.model.EMBED_CONTAINER_LABEL
import app.tatakai.next.model.EmbedCapture
import app.tatakai.next.model.EmbedSniffException
import app.tatakai.next.model.EmbedSniffer
import app.tatakai.next.model.ProviderEntryRef
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.StreamSource
import app.tatakai.next.model.WebInfo
import kotlinx.coroutines.test.runTest
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave B (16 §3.3 / §3.4, worker t): embed branch and provider branch of the source flow.
class SourceFlowWaveBTest {
    private fun embed(url: String = "https://embed.example/e/1") = SourceCandidate(
        extId = "toko",
        extName = "Toko",
        kind = SourceKind.WEB,
        label = "Vid",
        container = EMBED_CONTAINER_LABEL,
        web = WebInfo(url, headers = mapOf("Referer" to "https://toko.example/"), embed = true),
    )

    private class FakeSniffer(val result: () -> EmbedCapture) : EmbedSniffer {
        var calls = mutableListOf<Triple<String, String, Long>>()
        override suspend fun sniff(embedUrl: String, headers: Map<String, String>, extId: String, timeoutMs: Long): EmbedCapture {
            calls += Triple(embedUrl, extId, timeoutMs)
            return result()
        }
    }

    @Test
    fun embedCandidateIsSniffedWithTheUserTimeout() = runTest {
        val sniffer = FakeSniffer { EmbedCapture("https://cdn.example/master.m3u8", mapOf("Referer" to "https://embed.example/"), Container.HLS) }
        val stream = resolveWebStream(embed(), sniffer, SourcePrefs(embedTimeoutSecs = 7))
        assertEquals("https://cdn.example/master.m3u8", stream?.url)
        assertEquals(Container.HLS, stream?.container)
        assertEquals("toko", stream?.extensionId)
        assertEquals("Vid", stream?.provider)
        assertEquals(Triple("https://embed.example/e/1", "toko", 7_000L), sniffer.calls.single())
    }

    @Test
    fun timeoutIsClampedToTheAllowedRange() = runTest {
        val sniffer = FakeSniffer { EmbedCapture("https://cdn.example/a.mp4", emptyMap(), Container.PROGRESSIVE) }
        resolveWebStream(embed(), sniffer, SourcePrefs(embedTimeoutSecs = 600))
        assertEquals(60_000L, sniffer.calls.single().third)
    }

    @Test
    fun sniffFailuresSurfaceAsEmbedSniffException() = runTest {
        val failing = FakeSniffer { throw EmbedSniffException(EmbedSniffException.TIMEOUT) }
        val e = assertFailsWith<EmbedSniffException> { resolveWebStream(embed(), failing, SourcePrefs()) }
        assertEquals("embed: timeout", embedFailDetail(e))
        val none = assertFailsWith<EmbedSniffException> { resolveWebStream(embed(), null, SourcePrefs()) }
        assertEquals(EmbedSniffException.UNAVAILABLE, none.reason)
        val off = assertFailsWith<EmbedSniffException> { resolveWebStream(embed(), failing, SourcePrefs(allowEmbed = false)) }
        assertEquals(EmbedSniffException.BLOCKED, off.reason)
    }

    @Test
    fun directCandidateNeverTouchesTheSniffer() = runTest {
        val sniffer = FakeSniffer { error("must not sniff") }
        val direct = embed("https://cdn.example/x.m3u8").copy(container = "HLS", web = WebInfo("https://cdn.example/x.m3u8"))
        assertEquals("https://cdn.example/x.m3u8", resolveWebStream(direct, sniffer, SourcePrefs())?.url)
        assertTrue(sniffer.calls.isEmpty())
    }

    @Test
    fun providerStreamsKeepOrderAndGetUniqueKeys() {
        val streams = listOf(
            StreamSource("https://a.example/1.m3u8", emptyMap(), Container.HLS, "1080p", "Server", "myext"),
            StreamSource("https://a.example/2.m3u8", emptyMap(), Container.HLS, "720p", "Server", "myext"),
            StreamSource("https://a.example/3.mp4", emptyMap(), Container.PROGRESSIVE, null, "", "myext"),
        )
        val ranked = providerRanked(streams, "My Ext")
        assertEquals(listOf("Server", "Server 2", "My Ext"), ranked.map { it.candidate.label })
        assertEquals(3, ranked.map { it.key }.toSet().size)
        assertTrue(ranked.first().recommended)
        assertTrue(ranked.zipWithNext().all { (a, b) -> a.score > b.score })
        assertEquals("https://a.example/2.m3u8", ranked[1].candidate.toStream()?.url)
    }

    @Test
    fun providerHistoryUsesTheExtIdWithoutAnilist() {
        val ref = ProviderEntryRef("myext", "main", "item-9", "ep-3")
        assertEquals("ext:myext:main:item-9", historyAnimeId(null, null, ref))
        assertEquals("21", historyAnimeId(21, null, ref))
        assertNull(historyAnimeId(null, null, null))
        val e = progressEntry(null, null, ref, "Title", null, 3, 600_000, 1_200_000, Instant.EPOCH)
        assertEquals("ext:myext:main:item-9", e?.animeId)
        assertEquals("ext:myext:main:item-9?ep=3", e?.episodeId)
        assertNull(e?.anilistId)
        assertEquals("21", progressEntry(21, null, ref, "Title", null, 3, 600_000, 1_200_000, Instant.EPOCH)?.animeId)
    }
}
