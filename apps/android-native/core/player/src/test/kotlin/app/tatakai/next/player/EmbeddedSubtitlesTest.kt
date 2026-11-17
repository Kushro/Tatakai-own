// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import androidx.media3.common.MimeTypes
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import java.io.ByteArrayOutputStream
import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import kotlin.test.AfterTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

// 0.4.0 (sub-translation worker): embedded subtitle tracks read with Media3's MatroskaExtractor from a small MKV built in the test
// (EBML by hand: an ASS track, a SubRip track, a PGS track, three clusters), the sample -> cue mapping of every text format, track
// matching, torrent byte ranges and HLS WebVTT renditions.
class EmbeddedSubtitlesTest {
    private val files = mutableListOf<File>()

    @AfterTest
    fun cleanUp() = files.forEach { it.delete() }

    // ---- a tiny EBML writer ---------------------------------------------------------------------------------------------------------

    private fun idBytes(id: Long): ByteArray {
        var n = 1
        while (n < 4 && (id shr (8 * n)) != 0L) n++
        return ByteArray(n) { i -> (id shr (8 * (n - 1 - i))).toByte() }
    }

    /** Always 8-byte sizes (valid EBML, and what the cluster resync validation must accept). */
    private fun size8(size: Long): ByteArray = ByteArray(8) { i -> if (i == 0) 0x01 else (size shr (8 * (7 - i))).toByte() }

    private fun el(id: Long, vararg children: ByteArray): ByteArray {
        val body = ByteArrayOutputStream().apply { children.forEach { write(it) } }.toByteArray()
        return idBytes(id) + size8(body.size.toLong()) + body
    }

    private fun uint(id: Long, v: Long): ByteArray {
        var n = 1
        while (n < 8 && (v shr (8 * n)) != 0L) n++
        return el(id, ByteArray(n) { i -> (v shr (8 * (n - 1 - i))).toByte() })
    }

    private fun str(id: Long, s: String) = el(id, s.toByteArray(Charsets.UTF_8))

    private fun double(id: Long, d: Double) = el(id, ByteBuffer.allocate(8).putDouble(d).array())

    private fun block(track: Int, relMs: Int, data: String, durationMs: Long) = el(
        0xA0,
        el(0xA1, byteArrayOf((0x80 or track).toByte(), (relMs shr 8).toByte(), relMs.toByte(), 0) + data.toByteArray(Charsets.UTF_8)),
        uint(0x9B, durationMs),
    )

    private val assHeader = "[Script Info]\nScriptType: v4.00+\n\n[V4+ Styles]\nFormat: Name, Fontname\nStyle: Default,Arial\n\n" +
        "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n"

    private fun ass(text: String, style: String = "Default", layer: Int = 0) = "0,$layer,$style,,0,0,0,,$text"

    /** Returns the file bytes and the offsets of the three clusters. */
    private fun buildMkv(): Pair<ByteArray, List<Int>> {
        val header = el(
            0x1A45DFA3, uint(0x4286, 1), uint(0x42F7, 1), uint(0x42F2, 4), uint(0x42F3, 8), str(0x4282, "matroska"),
            uint(0x4287, 4), uint(0x4285, 2),
        )
        val info = el(0x1549A966, uint(0x2AD7B1, 1_000_000), double(0x4489, 420_000.0))
        val tracks = el(
            0x1654AE6B,
            el(0xAE, uint(0xD7, 1), uint(0x73C5, 11), uint(0x83, 0x11), str(0x86, "S_TEXT/ASS"), el(0x63A2, assHeader.toByteArray()), str(0x22B59C, "eng"), str(0x536E, "English [Full]")),
            el(0xAE, uint(0xD7, 2), uint(0x73C5, 12), uint(0x83, 0x11), str(0x86, "S_TEXT/UTF8"), str(0x22B59C, "spa"), str(0x536E, "Español")),
            el(0xAE, uint(0xD7, 3), uint(0x73C5, 13), uint(0x83, 0x11), str(0x86, "S_HDMV/PGS"), str(0x22B59C, "jpn")),
        )
        val padding = "x".repeat(3_000) // a fake "video" element between subtitle blocks (EBML Void), so clusters have some size
        val clusters = listOf(
            el(
                0x1F43B675, uint(0xE7, 0),
                block(1, 1_000, ass("{\\i1}Hello{\\i0}\\Nthere"), 2_000),
                block(1, 1_000, ass("{\\an8\\pos(10,10)}SIGN", "Sign", 1), 2_000),
                block(1, 1_000, ass("{\\an8\\pos(10,10)}SIGN", "Sign", 2), 2_000), // same event on another layer (outline)
                block(1, 2_000, ass("{\\p1}m 0 0 l 100 0 100 100 0 100{\\p0}", "Sign"), 3_000),
                block(2, 1_500, "<i>Hola</i>\nahí", 1_500),
                el(0xEC, padding.toByteArray()),
            ),
            el(
                0x1F43B675, uint(0xE7, 200_000),
                block(1, 0, ass("{\\k20}La{\\k20}la"), 1_000),
                el(0xEC, padding.toByteArray()),
                block(1, 500, ass("Second cluster line"), 1_500),
                el(0xEC, padding.toByteArray()),
            ),
            el(0x1F43B675, uint(0xE7, 400_000), block(1, 0, ass("Last one"), 2_000), el(0xEC, padding.toByteArray())),
        )
        val prefix = header.size + idBytes(0x18538067).size + 8 + info.size + tracks.size
        val offsets = clusters.runningFold(prefix) { acc, c -> acc + c.size }.dropLast(1)
        return header + el(0x18538067, info, tracks, *clusters.toTypedArray()) to offsets
    }

    private fun tempFile(bytes: ByteArray) = File.createTempFile("embedded", ".mkv").also {
        it.writeBytes(bytes)
        files += it
    }

    /** Wraps a source and records any read that touches a byte outside the available ranges (must never happen). */
    private class CheckingSource(private val inner: MediaByteSource, private val ranges: () -> List<LongRange>?) : MediaByteSource by inner {
        val violations = mutableListOf<Long>()

        private fun check(from: Long, n: Int) {
            if (n <= 0) return
            val r = ranges() ?: return
            if (r.none { from in it && from + n - 1 in it }) violations += from
        }

        override fun available(): List<LongRange>? = ranges()

        override fun openAt(position: Long): RawReader {
            val raw = inner.openAt(position)
            return object : RawReader() {
                var pos = position

                override fun read(buffer: ByteArray, offset: Int, length: Int): Int = raw.read(buffer, offset, length).also {
                    check(pos, it)
                    if (it > 0) pos += it
                }

                override fun close() = raw.close()
            }
        }

        override fun readAt(position: Long, buffer: ByteArray, offset: Int, length: Int): Int =
            inner.readAt(position, buffer, offset, length).also { check(position, it) }
    }

    private fun localSource(file: File, ranges: (() -> List<LongRange>?)? = null): MediaByteSource {
        val raf = RandomAccessFile(file, "r")
        return ChannelByteSource(raf.channel, raf, ranges)
    }

    @Test
    fun readsEveryTextTrackOfAnMkvWithMedia3() = runBlocking {
        val (bytes, _) = buildMkv()
        val ex = EmbeddedSubtitleExtractor(localSource(tempFile(bytes)), playheadMs = { 0 })
        withTimeout(20_000) { ex.run() }
        val st = ex.state.value
        assertTrue(st.finished)
        assertTrue(st.complete, "failure=${st.failure}")
        assertEquals(listOf(0, 1, 2), st.tracks.map { it.ordinal })
        assertEquals(listOf(MimeTypes.TEXT_SSA, MimeTypes.APPLICATION_SUBRIP, MimeTypes.APPLICATION_PGS), st.tracks.map { it.mime })
        assertTrue(st.tracks[2].image)
        assertEquals("English [Full]", st.tracks[0].label)

        val ass = st.cues.getValue(st.tracks[0].id)
        assertEquals(
            listOf(TimedCue(1_000, 3_000, "Hello\nthere"), TimedCue(1_000, 3_000, "SIGN"), TimedCue(200_500, 202_000, "Second cluster line"), TimedCue(400_000, 402_000, "Last one")),
            ass,
        )
        // Karaoke is kept out of translation (and the drawing produced no text at all).
        assertEquals(setOf("Lala"), st.skipped[st.tracks[0].id])
        assertEquals(listOf(TimedCue(1_500, 3_000, "Hola\nahí")), st.cues.getValue(st.tracks[1].id))
    }

    @Test
    fun partialTorrentReadsOnlyDownloadedBytesAndResumes() = runBlocking {
        val (bytes, clusters) = buildMkv()
        val file = tempFile(bytes)
        // Downloaded: everything up to the middle of the second cluster.
        val available = java.util.concurrent.atomic.AtomicReference(listOf(0L until (clusters[1] + 200).toLong()))
        val source = CheckingSource(localSource(file)) { available.get() }
        val ex = EmbeddedSubtitleExtractor(source, { 0 }, EmbeddedSubtitleExtractor.Policy(Long.MAX_VALUE, null, false, waitMs = 20))
        val job = async(Dispatchers.IO) { ex.run() }
        val waiting = withTimeout(10_000) { ex.state.first { it.waiting } }
        assertEquals(listOf("Hello\nthere", "SIGN"), waiting.cues.getValue(waiting.tracks[0].id).map { it.text })
        assertFalse(waiting.finished)
        // The rest arrives: reading resumes at the cluster that was cut and reaches the end.
        available.set(listOf(0L until bytes.size.toLong()))
        withTimeout(10_000) { job.await() }
        val st = ex.state.value
        assertTrue(st.complete, "failure=${st.failure}")
        assertEquals(listOf("Hello\nthere", "SIGN", "Second cluster line", "Last one"), st.cues.getValue(st.tracks[0].id).map { it.text })
        assertEquals(emptyList(), source.violations)
    }

    @Test
    fun aHoleInTheMiddleIsSkippedWhenTheViewerIsPastIt() = runBlocking {
        val (bytes, clusters) = buildMkv()
        // Downloaded: the start (header + cluster 1) and the end (cluster 3); cluster 2 is missing. The viewer is at 400 s.
        val available = listOf(0L until clusters[1].toLong(), clusters[2].toLong() until bytes.size.toLong())
        val source = CheckingSource(localSource(tempFile(bytes))) { available }
        val ex = EmbeddedSubtitleExtractor(source, { 400_000 }, EmbeddedSubtitleExtractor.Policy(Long.MAX_VALUE, null, false, waitMs = 20))
        val job = async(Dispatchers.IO) { ex.run() }
        val st = withTimeout(10_000) { ex.state.first { s -> s.cues.values.any { list -> list.any { it.text == "Last one" } } } }
        job.cancel()
        assertEquals(listOf("Hello\nthere", "SIGN", "Last one"), st.cues.getValue(st.tracks[0].id).map { it.text })
        assertEquals(emptyList(), source.violations)
    }

    @Test
    fun sampleMapping() {
        val ssa = SubtitleSamples.decode(MimeTypes.TEXT_SSA, 5_000, "Dialogue: 0:00:00:00,0:00:02:50,3,0,Default,,0,0,0,,{\\b1}Bold{\\b0} move".toByteArray())
        assertEquals(SampleCue(5_000, 7_500, "Bold move"), ssa)
        val srt = SubtitleSamples.decode(MimeTypes.APPLICATION_SUBRIP, 1_000, "1\n00:00:00,000 --> 00:00:01,250\n<b>Hi</b>\n".toByteArray())
        assertEquals(SampleCue(1_000, 2_250, "Hi"), srt)
        val vtt = SubtitleSamples.decode(MimeTypes.TEXT_VTT, 2_000, "WEBVTT\n\n00:00:00.000 --> 00:00:00.800\n<c.yellow>Hey</c>\n".toByteArray())
        assertEquals(SampleCue(2_000, 2_800, "Hey"), vtt)
        val text = "Line"
        val tx3g = byteArrayOf(0, text.length.toByte()) + text.toByteArray()
        assertEquals(SampleCue(3_000, null, "Line"), SubtitleSamples.decode(MimeTypes.APPLICATION_TX3G, 3_000, tx3g))
        assertEquals(SampleCue(4_000, null, ""), SubtitleSamples.decode(MimeTypes.APPLICATION_TX3G, 4_000, byteArrayOf(0, 0)))
        fun box(type: String, body: ByteArray) = ByteBuffer.allocate(8 + body.size).putInt(8 + body.size).put(type.toByteArray()).put(body).array()
        val wvtt = box("vttc", box("payl", "Boxed <i>cue</i>".toByteArray()))
        assertEquals(SampleCue(6_000, null, "Boxed cue"), SubtitleSamples.decode(MimeTypes.APPLICATION_MP4VTT, 6_000, wvtt))
        assertNull(SubtitleSamples.decode(MimeTypes.APPLICATION_PGS, 0, byteArrayOf(1, 2, 3)))
        assertEquals(1_234_560L, SubtitleSamples.ssaTime("0:20:34:56"))
        assertTrue(SubtitleSamples.isImage("hdmv_pgs_subtitle"))
        assertTrue(SubtitleSamples.isImage(MimeTypes.APPLICATION_VOBSUB))
        assertFalse(SubtitleSamples.isImage("ass"))
    }

    @Test
    fun trackMatching() {
        val tracks = listOf(
            EmbeddedTrack(1, 0, MimeTypes.TEXT_SSA, "eng", "Signs & Songs"),
            EmbeddedTrack(2, 1, MimeTypes.TEXT_SSA, "eng", "Full"),
            EmbeddedTrack(3, 2, MimeTypes.TEXT_SSA, "spa", "Español (LA)"),
            EmbeddedTrack(4, 3, MimeTypes.APPLICATION_PGS, "por", null),
        )
        // Same ordinal and language (mpv says "eng", Media3 says "en": both normalize).
        assertEquals(2, EmbeddedTrackMatcher.match(tracks, EmbeddedTrackRef(1, "en", "Full", "ass"))?.id)
        // Ordinal off by one (a track one side skipped): language wins.
        assertEquals(3, EmbeddedTrackMatcher.match(tracks, EmbeddedTrackRef(1, "spa", null, null))?.id)
        // Two candidates of the language: the title decides.
        assertEquals(1, EmbeddedTrackMatcher.match(tracks, EmbeddedTrackRef(3, "eng", "Signs & Songs", null))?.id)
        assertEquals(4, EmbeddedTrackMatcher.match(tracks, EmbeddedTrackRef(3, null, null, null))?.id)
        assertNull(EmbeddedTrackMatcher.match(emptyList(), EmbeddedTrackRef(0, null, null, null)))
    }

    @Test
    fun torrentFractionsBecomeInnerByteRanges() {
        assertEquals(listOf(0L until 250L, 500L until 1_000L), TorrentByteRanges.of(listOf(0.5..1.0, 0.0..0.25), 1_000))
        assertEquals(listOf(334L until 666L), TorrentByteRanges.of(listOf(0.3333..0.6666), 1_000))
        assertEquals(emptyList(), TorrentByteRanges.of(listOf(0.0..1.0), 0))
    }

    @Test
    fun hlsWebVttRenditions() {
        val master = "#EXTM3U\n" +
            "#EXT-X-MEDIA:TYPE=AUDIO,GROUP-ID=\"a\",NAME=\"Japanese\",URI=\"audio.m3u8\"\n" +
            "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"English\",LANGUAGE=\"en\",URI=\"subs/en.m3u8\"\n" +
            "#EXT-X-MEDIA:TYPE=SUBTITLES,GROUP-ID=\"subs\",NAME=\"Español\",LANGUAGE=\"es\",URI=\"https://cdn.example/es.m3u8\"\n" +
            "#EXT-X-STREAM-INF:BANDWIDTH=1,SUBTITLES=\"subs\"\nvideo.m3u8\n"
        val r = HlsSubtitles.renditions(master, "https://host.example/show/master.m3u8")
        assertEquals(listOf("https://host.example/show/subs/en.m3u8", "https://cdn.example/es.m3u8"), r.map { it.uri })
        assertEquals("es", HlsSubtitles.match(r, EmbeddedTrackRef(null, "spa", null, null))?.language)
        assertEquals("English", HlsSubtitles.match(r, EmbeddedTrackRef(0, null, "English", null))?.name)
        assertEquals(listOf("https://host.example/show/subs/seg0.vtt"), HlsSubtitles.segments("#EXTM3U\n#EXTINF:6,\nseg0.vtt\n#EXT-X-ENDLIST\n", r[0].uri))

        val seg1 = "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:900000,LOCAL:00:00:00.000\n\n00:00:01.000 --> 00:00:02.000\nFirst\n"
        val seg2 = "WEBVTT\nX-TIMESTAMP-MAP=MPEGTS:1440000,LOCAL:00:00:00.000\n\n00:00:00.500 --> 00:00:01.000\nSecond\n"
        val (c1, base) = HlsSubtitles.parseSegment(seg1, null)
        assertEquals(listOf(TimedCue(1_000, 2_000, "First")), c1)
        // Second segment's clock is 6 s later: its local 0.5 s is 6.5 s on the stream.
        assertEquals(listOf(TimedCue(6_500, 7_000, "Second")), HlsSubtitles.parseSegment(seg2, base).first)
        assertNotNull(base)
    }
}
