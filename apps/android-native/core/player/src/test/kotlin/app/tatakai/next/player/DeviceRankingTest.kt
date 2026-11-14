// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.Quality
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.VideoCodec
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

// BUG-017 / BUG-020 (plan §11.2): sources without a hardware decoder are marked and go down the ranking.
class DeviceRankingTest {
    /** moto g51: HEVC Main only in hardware, no Main10 anywhere, AV1 only in software. */
    private val motoG51 = CodecCaps(
        hevc = true, hevcMain10 = false, av1 = true, av1Main10 = true,
        hevcHw = true, hevcMain10Hw = false, av1Hw = false, av1Main10Hw = false,
    )

    private fun src(label: String, codec: VideoCodec?, bits: Int? = null, recommended: Boolean = false, picked: Boolean = false) =
        RankedSource(
            candidate = SourceCandidate(
                extId = "toko",
                extName = "Toko",
                kind = SourceKind.TORRENT,
                label = label,
                quality = Quality.P1080,
                torrent = TorrentInfo("a".repeat(40), seeders = 50),
                tags = ReleaseTags(codec = codec, bitDepth = bits),
            ),
            score = 0f,
            extEnabled = true,
            recommended = recommended,
            picked = picked,
        )

    @Test
    fun hardwareDecodingFollowsTheHardwareDecoders() {
        assertTrue(decodesInHardware(ReleaseTags(), motoG51), "web sources have no tags")
        assertTrue(decodesInHardware(ReleaseTags(codec = VideoCodec.H264, bitDepth = 10), motoG51))
        assertTrue(decodesInHardware(ReleaseTags(codec = VideoCodec.HEVC, bitDepth = 8), motoG51))
        assertFalse(decodesInHardware(ReleaseTags(codec = VideoCodec.HEVC, bitDepth = 10), motoG51))
        assertFalse(decodesInHardware(ReleaseTags(codec = VideoCodec.AV1), motoG51))
        assertTrue(isDeviceCompatible(ReleaseTags(codec = VideoCodec.AV1), motoG51), "AV1 still decodes (in software)")
        assertTrue(decodesInHardware(ReleaseTags(codec = VideoCodec.AV1, bitDepth = 10), CodecCaps.ALL))
    }

    @Test
    fun slowSourcesGoDownAndTheRecommendationMoves() {
        val list = listOf(
            src("ASW", VideoCodec.HEVC, 10, recommended = true),
            src("Judas", VideoCodec.HEVC, 10, picked = true),
            src("ToonsHub", VideoCodec.H264),
            src("Latino", VideoCodec.AV1),
            src("SubsPlease", VideoCodec.H264),
        )
        val out = rankForDevice(list, motoG51)
        assertEquals(listOf("ToonsHub", "SubsPlease", "ASW", "Judas", "Latino"), out.map { it.candidate.label })
        assertEquals("ToonsHub", out.single { it.recommended }.candidate.label)
        assertEquals("Judas", out.single { it.picked }.candidate.label, "the pick is kept")
    }

    @Test
    fun nothingChangesWhenEverythingIsFastOrEverythingIsSlow() {
        val list = listOf(src("A", VideoCodec.HEVC, 10, recommended = true), src("B", VideoCodec.H264))
        assertEquals(list, rankForDevice(list, CodecCaps.ALL))
        val allSlow = listOf(src("A", VideoCodec.HEVC, 10, recommended = true), src("B", VideoCodec.AV1))
        assertEquals(allSlow, rankForDevice(allSlow, motoG51))
    }
}
