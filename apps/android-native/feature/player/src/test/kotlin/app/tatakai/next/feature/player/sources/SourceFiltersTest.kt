// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player.sources

import app.tatakai.next.model.EpisodeRange
import app.tatakai.next.model.Quality
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.ReleaseSource
import app.tatakai.next.model.ReleaseTags
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.VideoCodec
import app.tatakai.next.model.WebInfo
import app.tatakai.next.player.CodecCaps
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

// Ported from the desktop tests of tk-ui/src/components/sources.rs.
class SourceFiltersTest {
    private fun vm(label: String, kind: SourceKind, quality: Quality, reliability: Float, tags: ReleaseTags) = RankedSource(
        candidate = SourceCandidate(
            extId = "ext",
            extName = "Ext",
            kind = kind,
            label = label,
            quality = quality,
            reliability = reliability,
            web = if (kind == SourceKind.WEB) WebInfo("https://cdn.example/$label") else null,
            torrent = if (kind == SourceKind.TORRENT) TorrentInfo("b".repeat(40)) else null,
            tags = tags,
        ),
        score = 0f,
    )

    private fun sample() = listOf(
        vm(
            "Judas", SourceKind.TORRENT, Quality.P1080, 0.95f,
            ReleaseTags(
                group = "Judas", source = ReleaseSource.BD, resolution = Quality.P1080, codec = VideoCodec.HEVC, bitDepth = 10,
                dualAudio = true, multiSub = true, batch = true, episodes = EpisodeRange(1, 12), seadexBest = true,
            ),
        ),
        vm("Alpha", SourceKind.WEB, Quality.P1080, 0.8f, ReleaseTags()),
        vm("SubsPlease", SourceKind.TORRENT, Quality.P720, 0.5f, ReleaseTags(group = "SubsPlease", source = ReleaseSource.WEB, resolution = Quality.P720)),
    )

    private fun labels(list: List<RankedSource>) = list.map { it.candidate.label }

    @Test
    fun defaultFiltersKeepEverythingInOrder() {
        val f = SourceFilters()
        assertEquals(listOf("Judas", "Alpha", "SubsPlease"), labels(f.apply(sample())))
        assertEquals(0, f.active())
    }

    @Test
    fun filtersCombineAndNeverReorder() {
        val list = sample()
        var f = SourceFilters(kind = SourceKind.TORRENT)
        assertEquals(listOf("Judas", "SubsPlease"), labels(f.apply(list)))
        f = f.copy(sources = f.sources.toggled(ReleaseSource.WEB).toggled(ReleaseSource.BD))
        assertEquals(listOf("Judas", "SubsPlease"), labels(f.apply(list)))
        f = f.copy(sources = f.sources.toggled(ReleaseSource.WEB))
        assertEquals(listOf("Judas"), labels(f.apply(list)))
        assertEquals(1, f.active())
        f = SourceFilters(resolutions = setOf(Quality.P1080))
        assertEquals(listOf("Judas", "Alpha"), labels(f.apply(list)))
        f = f.copy(tenBit = true)
        assertEquals(listOf("Judas"), labels(f.apply(list)))
        f = SourceFilters(batch = true, seadexBest = true, dualAudio = true, multiSub = true)
        assertEquals(listOf("Judas"), labels(f.apply(list)))
        assertEquals(4, f.active())
        assertTrue(SourceFilters(codecs = setOf(VideoCodec.AV1)).apply(list).isEmpty())
    }

    @Test
    fun reliabilityFloorToggles() {
        val list = sample()
        var f = SourceFilters().toggleFloor(RelFloor.MID)
        assertEquals(listOf("Judas", "Alpha"), labels(f.apply(list)))
        f = f.toggleFloor(RelFloor.HIGH)
        assertEquals(listOf("Judas"), labels(f.apply(list)))
        f = f.toggleFloor(RelFloor.HIGH)
        assertEquals(null, f.minReliability)
        assertEquals(3, f.apply(list).size)
    }

    @Test
    fun deviceFilterHidesWhatTheDecodersCannotPlay() {
        val noHevc10 = CodecCaps(hevc = true, hevcMain10 = false, av1 = true, av1Main10 = true)
        val f = SourceFilters(deviceCompatible = true)
        assertEquals(listOf("Alpha", "SubsPlease"), labels(f.apply(sample(), noHevc10)))
        assertEquals(3, f.apply(sample(), CodecCaps.ALL).size)
        assertTrue(FilterOptions.of(sample(), noHevc10).deviceIncompatible)
    }

    @Test
    fun optionsOnlyOfferWhatTheListHas() {
        val o = FilterOptions.of(sample())
        assertEquals(listOf(ReleaseSource.BD, ReleaseSource.WEB), o.sources)
        assertEquals(listOf(Quality.P1080, Quality.P720), o.resolutions)
        assertEquals(listOf(VideoCodec.HEVC), o.codecs)
        assertTrue(o.tenBit && o.dualAudio && o.multiSub && o.batch && o.seadexBest && o.anyFeature)
        assertEquals(FilterOptions(), FilterOptions.of(emptyList()))
    }

    @Test
    fun tagChipsFollowTheTags() {
        val list = sample()
        val chips = tagChips(list[0])
        // SeaDex best, (group == label: no chip), BD, 1080p, HEVC, 10-bit, dual audio, multi-sub, batch
        assertEquals(
            listOf(
                TagChip.SeadexBest, TagChip.Code("BD"), TagChip.Code("1080p"), TagChip.Code("HEVC"), TagChip.BitDepth(10),
                TagChip.DualAudio, TagChip.MultiSub, TagChip.Batch(1, 12),
            ),
            chips,
        )
        val other = list[2].copy(candidate = list[2].candidate.copy(label = "SubsPlease 720p"))
        assertEquals(TagChip.Group("SubsPlease"), tagChips(other)[0])
        assertTrue(tagChips(list[1]).isEmpty(), "web rows without tags get no chips")
    }

    @Test
    fun saveableEncodingRoundTrips() {
        val f = SourceFilters(
            kind = SourceKind.WEB, sources = setOf(ReleaseSource.BD), resolutions = setOf(Quality.P720), codecs = setOf(VideoCodec.HEVC),
            tenBit = true, batch = true, minReliability = RelFloor.HIGH, deviceCompatible = true,
        )
        assertEquals(f, SourceFilters.decode(f.encode()))
        assertEquals(SourceFilters(), SourceFilters.decode(SourceFilters().encode()))
        assertEquals(SourceFilters(kind = SourceKind.WEB), f.cleared())
    }

    @Test
    fun reliabilityTonesAndLanguages() {
        assertEquals(RelTone.HIGH, reliabilityTone(0.9f))
        assertEquals(RelTone.MID, reliabilityTone(0.7f))
        assertEquals(RelTone.LOW, reliabilityTone(0.69f))
        assertEquals("ES · EN", langsLabel(listOf("es", "en")))
        assertEquals(null, langsLabel(emptyList()))
    }
}
