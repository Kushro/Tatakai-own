// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Port of the sources.rs unit tests that live in :core:model (Quality::parse, from_stream / to_stream, source keys). */
class SourcesTest {
    private fun stream(quality: String?) = StreamSource(
        url = "https://cdn.example/ep.m3u8",
        headers = mapOf("Referer" to "https://site.example/"),
        container = Container.HLS,
        quality = quality,
        provider = "Alpha",
        extensionId = "io.github.toko",
        subtitles = listOf(
            SubtitleTrack("https://cdn.example/es.vtt", "es-419", "Español", "vtt"),
            SubtitleTrack("https://cdn.example/x.vtt", "und", "", "vtt"),
            SubtitleTrack("https://cdn.example/en.vtt", "EN", "English", "vtt"),
        ),
    )

    @Test
    fun qualityParseIsLenient() {
        val cases = listOf(
            "1080p" to Quality.P1080, "1080" to Quality.P1080, "1920x1080" to Quality.P1080, "FHD" to Quality.P1080,
            "Full HD" to Quality.P1080, "4K" to Quality.P2160, "2160p" to Quality.P2160, "UHD" to Quality.P2160,
            "1440p" to Quality.P1440, "720p" to Quality.P720, "HD" to Quality.P720, "480p" to Quality.P480, "SD" to Quality.P480,
            "" to Quality.UNKNOWN, "auto" to Quality.UNKNOWN, "default" to Quality.UNKNOWN,
        )
        for ((s, q) in cases) assertEquals(s, q, Quality.parse(s))
        assertEquals(Quality.UNKNOWN, Quality.parse(null))
        assertTrue(Quality.UNKNOWN < Quality.P480 && Quality.P1080 < Quality.P2160)
        assertEquals("\"720p\"", Json.encodeToString(Quality.serializer(), Quality.P720))
        assertEquals(Quality.UNKNOWN, Json.decodeFromString(Quality.serializer(), "\"unknown\""))
    }

    @Test
    fun fromStreamAndBack() {
        val s = stream("1080p")
        val c = SourceCandidate.fromStream(s, "Toko")
        assertEquals(SourceKind.WEB, c.kind)
        assertEquals("Alpha", c.label)
        assertEquals("Toko", c.extName)
        assertEquals(Quality.P1080, c.quality)
        assertEquals("HLS", c.container)
        assertEquals(listOf("es", "en"), c.subs)
        assertTrue(c.dubs.isEmpty() && c.note == null && c.torrent == null)
        assertEquals(INITIAL_RELIABILITY, c.reliability)
        assertNull(c.web?.latencyMs)
        assertEquals(s, c.toStream())
        val none = SourceCandidate.fromStream(stream(null), "Toko")
        assertEquals(Quality.UNKNOWN, none.quality)
        assertNull(none.toStream()!!.quality)
    }

    @Test
    fun toStreamFallsBackToTheUrlAndNeverPlaysTorrents() {
        var c = SourceCandidate.fromStream(stream("720p"), "Toko").copy(container = "HEVC")
        assertEquals(Container.HLS, c.toStream()!!.container)
        c = c.copy(web = c.web!!.copy(url = "https://cdn.example/watch"))
        assertNull(c.toStream())
        val t = SourceCandidate.fromStream(stream("720p"), "Nyaa")
            .copy(kind = SourceKind.TORRENT, torrent = TorrentInfo(infoHash = "ab".repeat(20), seeders = 10, leechers = 1))
        assertNull(t.toStream())
        assertEquals("magnet:?xt=urn:btih:${"ab".repeat(20)}", t.torrentUri())
    }

    @Test
    fun sourceKeyRoundTrip() {
        var c = SourceCandidate.fromStream(stream(null), "Toko")
        assertEquals("io.github.toko|web|Alpha", sourceKey(c))
        assertEquals(Triple("io.github.toko", SourceKind.WEB, "Alpha"), parseSourceKey("io.github.toko|web|Alpha"))
        c = c.copy(label = "odd|label", kind = SourceKind.TORRENT)
        assertEquals(Triple("io.github.toko", SourceKind.TORRENT, "odd|label"), parseSourceKey(sourceKey(c)))
        assertEquals(Triple("x", SourceKind.WEB, ""), parseSourceKey("x|web|"))
        for (bad in listOf("", "x", "x|web", "x|ftp|y", "|web|y")) assertNull(bad, parseSourceKey(bad))
    }
}
