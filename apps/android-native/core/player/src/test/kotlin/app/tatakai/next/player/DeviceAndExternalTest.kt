// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.Container
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.SubtitleTrack
import app.tatakai.next.model.VideoCodec
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class DeviceAndExternalTest {
    @Test
    fun deviceCompatibilityFollowsTheDecoders() {
        val noTenBit = CodecCaps(hevc = true, hevcMain10 = false, av1 = false, av1Main10 = false)
        assertTrue(isDeviceCompatible(ReleaseTags(), noTenBit), "web sources have no tags")
        assertTrue(isDeviceCompatible(ReleaseTags(codec = VideoCodec.HEVC, bitDepth = 8), noTenBit))
        assertFalse(isDeviceCompatible(ReleaseTags(codec = VideoCodec.HEVC, bitDepth = 10), noTenBit))
        assertFalse(isDeviceCompatible(ReleaseTags(codec = VideoCodec.AV1), noTenBit))
        assertTrue(isDeviceCompatible(ReleaseTags(codec = VideoCodec.H264, bitDepth = 10), noTenBit))
        assertTrue(isDeviceCompatible(ReleaseTags(codec = VideoCodec.AV1, bitDepth = 10), CodecCaps.ALL))
    }

    @Test
    fun externalLaunchCarriesHeadersTitleAndStart() {
        val src = PlayableSource(
            url = "https://cdn.example/ep1.m3u8",
            headers = mapOf("Referer" to "https://site.example/", "User-Agent" to "UA"),
            container = Container.HLS,
            label = "Toko - 1080p",
            subtitles = listOf(SubtitleTrack("https://cdn.example/es.vtt", "es", "Español", "vtt")),
        )
        val l = ExternalPlayer.launch(src, "Frieren · Episode 1", 65_000, ExternalPlayer.PACKAGE_MPV)
        assertEquals("application/x-mpegURL", l.mimeType)
        assertEquals("is.xyz.mpv", l.packageName)
        assertEquals("Frieren · Episode 1", l.extras["title"])
        assertEquals(65_000, l.extras["position"])
        assertEquals(65_000L, l.extras["extra_position"])
        assertContentEquals(listOf("Referer", "https://site.example/", "User-Agent", "UA"), (l.extras["headers"] as Array<*>).toList())
        assertEquals("Referer: https://site.example/\r\nUser-Agent: UA", l.extras["http-headers"])
        assertContentEquals(listOf("https://cdn.example/es.vtt"), (l.extras["subs"] as Array<*>).toList())

        val loopback = PlayableSource("http://127.0.0.1:4000/t/hash/3/Ep%2003.mkv", emptyMap(), Container.PROGRESSIVE, "torrent", torrent = true)
        val t = ExternalPlayer.launch(loopback, "x", 0, "  ")
        assertEquals("video/x-matroska", t.mimeType)
        assertNull(t.packageName, "blank package = system chooser")
        assertFalse("headers" in t.extras)
        assertFalse("position" in t.extras)
    }
}
