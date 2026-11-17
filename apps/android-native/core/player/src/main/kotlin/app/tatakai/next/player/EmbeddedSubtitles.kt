// SPDX-License-Identifier: AGPL-3.0-or-later
@file:androidx.annotation.OptIn(markerClass = [androidx.media3.common.util.UnstableApi::class])

package app.tatakai.next.player

import androidx.media3.common.C
import androidx.media3.common.DataReader
import androidx.media3.common.Format
import androidx.media3.common.MimeTypes
import androidx.media3.common.ParserException
import androidx.media3.common.util.ParsableByteArray
import androidx.media3.extractor.DefaultExtractorInput
import androidx.media3.extractor.DiscardingTrackOutput
import androidx.media3.extractor.Extractor
import androidx.media3.extractor.ExtractorInput
import androidx.media3.extractor.ExtractorOutput
import androidx.media3.extractor.PositionHolder
import androidx.media3.extractor.SeekMap
import androidx.media3.extractor.TrackOutput
import androidx.media3.extractor.mkv.MatroskaExtractor
import androidx.media3.extractor.mp4.FragmentedMp4Extractor
import androidx.media3.extractor.mp4.Mp4Extractor
import androidx.media3.extractor.text.SubtitleParser as Media3SubtitleParser
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.withContext
import java.io.Closeable
import java.io.EOFException
import java.io.IOException
import java.nio.ByteBuffer
import java.nio.channels.FileChannel
import kotlin.coroutines.coroutineContext
import kotlin.math.abs
import kotlin.math.min

// 0.4.0 (sub-translation worker): the full cue list of an EMBEDDED subtitle track (MKV / WebM / MP4), so pre-translation and
// look-ahead work for the common case (torrent MKVs with ASS tracks on libmpv, Media3 MKV / MP4) and not only for sidecar files.
//
// Media3's own extractors read the container (MatroskaExtractor / FragmentedMp4Extractor / Mp4Extractor with
// FLAG_EMIT_RAW_SUBTITLE_DATA, so text samples arrive as the container stores them, not transcoded to Media3 cues); an ExtractorOutput
// keeps only text tracks and discards every audio / video sample. Samples become [TimedCue]s via [SubtitleSamples].
//
// Sources ([MediaByteSource]): a local file / content URI, the torrent's loopback HTTP stream limited to the downloaded byte ranges
// (never asks the torrent for a piece it does not have: reading stops at a missing range, waits, and resyncs at the next Matroska
// Cluster), or a remote HTTP URL (Range requests, starts near the playhead and stays at most a few minutes ahead of it, with a byte
// budget, so it never downloads the whole file next to the player).

/** The next byte is not downloaded yet (torrent): the driver waits and resyncs. */
class DataNotAvailableException(val position: Long) : IOException("byte $position not available yet")

private class BudgetExceededException : IOException("byte budget exhausted")

/** Sequential reader of a [MediaByteSource] from one position. */
abstract class RawReader : Closeable {
    /** Like `InputStream.read`; -1 at the end. */
    abstract fun read(buffer: ByteArray, offset: Int, length: Int): Int
}

/** Random-access bytes of a media file with what is readable right now. */
interface MediaByteSource : Closeable {
    /** Total size; -1 until [prepare] (or when unknown). */
    val length: Long

    /** Reading costs network data (pacing + budget apply). */
    val remote: Boolean

    /** Resolves [length] (an HTTP probe); may throw [IOException]. */
    suspend fun prepare() {}

    /** Byte ranges readable now without waiting; null = all of it. */
    fun available(): List<LongRange>? = null

    fun openAt(position: Long): RawReader

    /** One read at [position] (scanning for a resync point); -1 at the end. */
    fun readAt(position: Long, buffer: ByteArray, offset: Int, length: Int): Int
}

/** A local file / content URI ([FileChannel]); [availability] limits it to what a torrent has downloaded (null = complete). */
class ChannelByteSource(
    private val channel: FileChannel,
    private val onClose: Closeable? = null,
    private val availability: (() -> List<LongRange>?)? = null,
) : MediaByteSource {
    override val length: Long get() = runCatching { channel.size() }.getOrDefault(-1L)
    override val remote = false

    override fun available(): List<LongRange>? = availability?.invoke()

    override fun openAt(position: Long): RawReader = object : RawReader() {
        private var pos = position

        override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
            val n = channel.read(ByteBuffer.wrap(buffer, offset, length), pos)
            if (n > 0) pos += n
            return n
        }

        override fun close() {}
    }

    override fun readAt(position: Long, buffer: ByteArray, offset: Int, length: Int): Int = channel.read(ByteBuffer.wrap(buffer, offset, length), position)

    override fun close() {
        runCatching { channel.close() }
        runCatching { onClose?.close() }
    }
}

/** What one embedded subtitle track is (container order = [ordinal] among the container's subtitle tracks). */
data class EmbeddedTrack(
    val id: Int,
    val ordinal: Int,
    val mime: String?,
    val language: String?,
    val label: String?,
) {
    val image: Boolean get() = SubtitleSamples.isImage(mime)
    val text: Boolean get() = SubtitleSamples.isText(mime)
}

/** The player's selected track, as the engine describes it (see [SubtitleOption.embeddedOrdinal] / [SubtitleOption.codec]). */
data class EmbeddedTrackRef(val ordinal: Int?, val language: String?, val title: String?, val codec: String?)

/** Snapshot of an extraction. [cues] / [skipped] per [EmbeddedTrack.id]; [complete] = the whole file was read. */
data class EmbeddedSubtitlesState(
    val tracks: List<EmbeddedTrack> = emptyList(),
    val cues: Map<Int, List<TimedCue>> = emptyMap(),
    val skipped: Map<Int, Set<String>> = emptyMap(),
    val complete: Boolean = false,
    /** Blocked on bytes the torrent has not downloaded yet (everything available so far is read). */
    val waiting: Boolean = false,
    /** Stopped for good without reading everything (budget, unsupported container, broken file); null = fine. */
    val failure: String? = null,
    val finished: Boolean = false,
)

/** Track matching between the engine's list and the container's (pure). */
object EmbeddedTrackMatcher {
    /**
     * The container track the player selected: same [EmbeddedTrackRef.ordinal] (both engines and Media3's extractors list tracks in
     * container order) when its language agrees; else the one with the same language (+ title); else the ordinal / the only track.
     */
    fun match(tracks: List<EmbeddedTrack>, want: EmbeddedTrackRef): EmbeddedTrack? {
        if (tracks.isEmpty()) return null
        val wantLang = LanguageCodes.normalize(want.language)
        fun langOf(t: EmbeddedTrack) = LanguageCodes.normalize(t.language)
        val byOrdinal = want.ordinal?.let { o -> tracks.firstOrNull { it.ordinal == o } }
        if (byOrdinal != null && (wantLang == null || langOf(byOrdinal) == null || langOf(byOrdinal) == wantLang)) return byOrdinal
        if (wantLang != null) {
            val same = tracks.filter { langOf(it) == wantLang }
            same.firstOrNull { !want.title.isNullOrBlank() && it.label == want.title }?.let { return it }
            if (same.size == 1) return same[0]
            if (same.size > 1) return same.minBy { abs(it.ordinal - (want.ordinal ?: 0)) }
        }
        return byOrdinal ?: tracks.singleOrNull()
    }
}

/** One subtitle event decoded from a container sample. [endMs] null = until the next sample (tx3g / MP4 WebVTT). */
data class SampleCue(val startMs: Long, val endMs: Long?, val text: String, val karaoke: Boolean = false, val drawing: Boolean = false)

/** Container text samples -> [SampleCue] (pure, no Android). */
object SubtitleSamples {
    private val IMAGE = setOf(MimeTypes.APPLICATION_PGS, MimeTypes.APPLICATION_VOBSUB, MimeTypes.APPLICATION_DVBSUBS, "image/bmp", "image/png")
    private val TEXT = setOf(MimeTypes.TEXT_SSA, MimeTypes.APPLICATION_SUBRIP, MimeTypes.TEXT_VTT, MimeTypes.APPLICATION_TX3G, MimeTypes.APPLICATION_MP4VTT)

    /** mpv codec names of bitmap subtitles. */
    private val IMAGE_CODECS = setOf("hdmv_pgs_subtitle", "pgssub", "dvd_subtitle", "dvdsub", "dvb_subtitle", "dvbsub", "xsub", "pgs", "vobsub")

    fun isImage(mimeOrCodec: String?): Boolean = mimeOrCodec != null && (mimeOrCodec in IMAGE || mimeOrCodec.lowercase() in IMAGE_CODECS)

    fun isText(mime: String?): Boolean = mime != null && mime in TEXT

    /**
     * MatroskaExtractor (raw) writes SSA blocks as `Dialogue: 0:00:00:00,<duration h:mm:ss:cc>,ReadOrder,Layer,Style,Name,MarginL,
     * MarginR,MarginV,Effect,Text`, SubRip as `1\n00:00:00,000 --> <duration>\n<text>`, WebVTT as `WEBVTT\n\n00:00:00.000 --> <duration>
     * \n<text>`: times relative to the sample time. MP4 tx3g = u16 length + text; MP4 WebVTT = `vttc` boxes with a `payl` child.
     */
    fun decode(mime: String?, sampleMs: Long, data: ByteArray): SampleCue? = when (mime) {
        MimeTypes.TEXT_SSA -> ssa(sampleMs, String(data, Charsets.UTF_8))
        MimeTypes.APPLICATION_SUBRIP -> block(sampleMs, String(data, Charsets.UTF_8), "srt")
        MimeTypes.TEXT_VTT -> block(sampleMs, String(data, Charsets.UTF_8), "vtt")
        MimeTypes.APPLICATION_TX3G -> tx3g(sampleMs, data)
        MimeTypes.APPLICATION_MP4VTT -> mp4Vtt(sampleMs, data)
        else -> null
    }

    private fun ssa(sampleMs: Long, s: String): SampleCue? {
        val body = s.trimStart('﻿').substringAfter("Dialogue:", missingDelimiterValue = "").trimStart()
        if (body.isEmpty()) return null
        val f = body.split(',', limit = 11)
        if (f.size < 11) return null
        val start = ssaTime(f[0]) ?: 0L
        val end = ssaTime(f[1]) ?: return null
        val t = SubtitleText.fromAss(f[10].trimEnd('\r', '\n'))
        return SampleCue(sampleMs + start, sampleMs + end, t.plain, karaoke = t.karaoke, drawing = t.drawingOnly)
    }

    /** `h:mm:ss:cc` (Media3's raw SSA) or `h:mm:ss.cc` (files). */
    internal fun ssaTime(s: String): Long? {
        val p = s.trim().split(':', '.')
        if (p.size != 4) return null
        val h = p[0].toLongOrNull() ?: return null
        val m = p[1].toLongOrNull() ?: return null
        val sec = p[2].toLongOrNull() ?: return null
        val cs = p[3].take(2).padEnd(2, '0').toLongOrNull() ?: return null
        return ((h * 60 + m) * 60 + sec) * 1000 + cs * 10
    }

    private fun block(sampleMs: Long, s: String, format: String): SampleCue? {
        val cue = SubtitleParser.parse(s, format).firstOrNull() ?: return null
        return SampleCue(sampleMs + cue.startMs, sampleMs + cue.endMs, cue.text)
    }

    private fun tx3g(sampleMs: Long, data: ByteArray): SampleCue? {
        if (data.size < 2) return SampleCue(sampleMs, null, "")
        val len = ((data[0].toInt() and 0xFF) shl 8) or (data[1].toInt() and 0xFF)
        val n = min(len, data.size - 2)
        if (n <= 0) return SampleCue(sampleMs, null, "")
        val utf16 = n >= 2 && ((data[2] == 0xFE.toByte() && data[3] == 0xFF.toByte()) || (data[2] == 0xFF.toByte() && data[3] == 0xFE.toByte()))
        val text = String(data, 2, n, if (utf16) Charsets.UTF_16 else Charsets.UTF_8)
        return SampleCue(sampleMs, null, SubtitleParser.clean(text))
    }

    private fun mp4Vtt(sampleMs: Long, data: ByteArray): SampleCue {
        val parts = ArrayList<String>()
        fun boxes(from: Int, to: Int, visit: (type: String, start: Int, end: Int) -> Unit) {
            var p = from
            while (p + 8 <= to) {
                val size = ((data[p].toInt() and 0xFF) shl 24) or ((data[p + 1].toInt() and 0xFF) shl 16) or
                    ((data[p + 2].toInt() and 0xFF) shl 8) or (data[p + 3].toInt() and 0xFF)
                if (size < 8 || p + size > to) break
                visit(String(data, p + 4, 4, Charsets.US_ASCII), p + 8, p + size)
                p += size
            }
        }
        boxes(0, data.size) { type, s, e ->
            if (type == "vttc") boxes(s, e) { inner, s2, e2 -> if (inner == "payl") parts += String(data, s2, e2 - s2, Charsets.UTF_8) }
        }
        return SampleCue(sampleMs, null, SubtitleParser.clean(parts.joinToString("\n")))
    }
}

/**
 * Reads the subtitle tracks of one media file in the background. Call [run] in a coroutine (it returns when the file is fully read,
 * the budget is spent, or it is cancelled); observe [state]. [playheadMs] drives pacing (remote) and jumps (seek past the read part).
 */
class EmbeddedSubtitleExtractor(
    private val source: MediaByteSource,
    private val playheadMs: () -> Long,
    private val policy: Policy = Policy.forSource(source),
) {
    /**
     * [maxBytes] total read budget; [maxAheadMs] stop reading when the cues read are this far ahead of the playhead (null = read as fast
     * as possible); [startNearPlayhead] = jump to the playhead after the header instead of reading from the start (remote files).
     */
    data class Policy(val maxBytes: Long, val maxAheadMs: Long?, val startNearPlayhead: Boolean, val waitMs: Long = WAIT_MS) {
        companion object {
            fun forSource(source: MediaByteSource) =
                if (source.remote) Policy(maxBytes = 1_500L * 1024 * 1024, maxAheadMs = 6 * 60_000L, startNearPlayhead = true)
                else Policy(maxBytes = Long.MAX_VALUE, maxAheadMs = null, startNearPlayhead = false)
        }
    }

    private val _state = MutableStateFlow(EmbeddedSubtitlesState())
    val state: StateFlow<EmbeddedSubtitlesState> = _state.asStateFlow()

    private val collectors = LinkedHashMap<Int, TextCollector>()
    private val discard = DiscardingTrackOutput()
    private var tracksEnded = false
    private var durationUs = C.TIME_UNSET
    private var firstClusterPos = -1L
    private var bytesRead = 0L
    private var lastSampleMs = -1L
    private var runStartMs = 0L
    private var dirtyCues = 0
    private var lastPublish = 0L

    private enum class Container { MKV, FMP4, MP4 }

    suspend fun run() = withContext(Dispatchers.IO) {
        try {
            source.prepare()
            val container = sniff() ?: return@withContext finish(failure = "unsupported container")
            when (container) {
                Container.MKV -> runMatroska()
                else -> runMp4(container)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: BudgetExceededException) {
            finish(failure = "budget")
        } catch (e: Exception) {
            finish(failure = e.message ?: e.javaClass.simpleName)
        } finally {
            activeReader?.close()
            runCatching { source.close() }
        }
    }

    // ---- driver -------------------------------------------------------------------------------------------------------------------

    private suspend fun sniff(): Container? {
        while (true) {
            coroutineContext.ensureActive()
            try {
                if (sniffs(MatroskaExtractor(Media3SubtitleParser.Factory.UNSUPPORTED, MKV_FLAGS))) return Container.MKV
                if (sniffs(FragmentedMp4Extractor(Media3SubtitleParser.Factory.UNSUPPORTED, FMP4_FLAGS))) return Container.FMP4
                if (sniffs(Mp4Extractor(Media3SubtitleParser.Factory.UNSUPPORTED, MP4_FLAGS))) return Container.MP4
                return null
            } catch (e: DataNotAvailableException) {
                waitForData()
            }
        }
    }

    private fun sniffs(extractor: Extractor): Boolean = try {
        extractor.sniff(input(0))
    } catch (e: EOFException) {
        false
    } catch (e: ParserException) {
        false
    }

    private var activeReader: Reader? = null

    /** A fresh input at [position] (after a seek / resync); the previous reader's connection is closed. */
    private fun input(position: Long): ExtractorInput {
        activeReader?.close()
        val reader = Reader(position).also { activeReader = it }
        return DefaultExtractorInput(reader, position, source.length.takeIf { it >= 0 } ?: C.LENGTH_UNSET.toLong())
    }

    private suspend fun waitForData() {
        publish(force = true, waiting = true)
        delay(policy.waitMs)
    }

    private suspend fun runMatroska() {
        fun newExtractor() = MatroskaExtractor(Media3SubtitleParser.Factory.UNSUPPORTED, MKV_FLAGS).also { it.init(Output()) }
        var extractor = newExtractor()
        val holder = PositionHolder()
        var input = input(0)
        var gapped = false
        var startDecided = false
        var resyncFailures = 0
        var lastFarCheck = 0L
        while (true) {
            coroutineContext.ensureActive()
            val result = try {
                extractor.read(input, holder)
            } catch (e: DataNotAvailableException) {
                if (!tracksEnded) {
                    // The header itself is not downloaded yet: start over once it is.
                    waitForData()
                    resetPartials()
                    extractor = newExtractor()
                    input = input(0)
                    continue
                }
                // The extractor's state is mid-element: never read on from here, only from a cluster start (resumePoint waits).
                var at: Long?
                do {
                    at = resumePoint(e.position)
                } while (at == null)
                if (at > e.position) gapped = true
                resetPartials()
                extractor.seek(at, 0)
                input = input(at)
                continue
            } catch (e: ParserException) {
                // A hole the torrent reported as downloaded, or damage: skip to the next cluster a few times, then give up.
                if (!tracksEnded || ++resyncFailures > MAX_RESYNCS) throw e
                val at = findClusterForward(input.position + 1, RESYNC_SCAN_BYTES) ?: throw e
                gapped = true
                resetPartials()
                extractor.seek(at, 0)
                input = input(at)
                continue
            }
            when (result) {
                Extractor.RESULT_SEEK -> input = input(holder.position)
                Extractor.RESULT_END_OF_INPUT -> {
                    flushOpenCues()
                    return finish(complete = !gapped)
                }
            }
            publish()
            if (!tracksEnded || firstClusterPos < 0) continue
            // Remote: after the header, start where the viewer is (the beginning is behind them).
            if (policy.startNearPlayhead && !startDecided) {
                startDecided = true
                val target = (playheadMs() - JUMP_BACK_MS).coerceAtLeast(0)
                if (target > FAR_MS) {
                    jumpTo(target, input.position)?.let { at ->
                        gapped = true
                        resetPartials()
                        extractor.seek(at, 0)
                        input = input(at)
                        runStartMs = target
                    }
                }
            }
            if (policy.maxAheadMs != null) paceRemote()
            // Remote: the viewer seeked far past what this run reached: move there. (Local / torrent data is read at disk speed; a
            // torrent blocked on a missing piece jumps in resumePoint.)
            val now = System.nanoTime() / 1_000_000
            if (policy.maxAheadMs != null && now - lastFarCheck > FAR_CHECK_MS) {
                lastFarCheck = now
                val target = (playheadMs() - JUMP_BACK_MS).coerceAtLeast(0)
                if (target > maxOf(lastSampleMs, 0) + FAR_MS) {
                    jumpTo(target, input.position)?.let { at ->
                        gapped = true
                        resetPartials()
                        extractor.seek(at, 0)
                        input = input(at)
                        runStartMs = target
                    }
                }
            }
        }
    }

    /** Remote: wait while the cues read are [Policy.maxAheadMs] ahead of the playhead. */
    private suspend fun paceRemote() {
        val ahead = policy.maxAheadMs ?: return
        while (lastSampleMs >= 0 && lastSampleMs > playheadMs() + ahead) {
            publish(force = true)
            delay(policy.waitMs)
            if (playheadMs() < runStartMs - FAR_MS) return // seeked back before this run: let the jump logic decide
        }
    }

    /**
     * Where to continue after [failedAt] was not downloaded: the cluster containing it once the torrent has it (re-reading a few
     * already seen events is harmless: cues are de-duplicated), or a cluster near the playhead when the viewer is far ahead of the
     * download frontier we are blocked on. Null = still waiting (it delays).
     */
    private suspend fun resumePoint(failedAt: Long): Long? {
        val ranges = source.available() ?: return failedAt
        val here = ranges.firstOrNull { failedAt in it }
        val total = source.length
        fun usable(r: LongRange, from: Long) = r.last - from >= MIN_RESUME_BYTES || (total > 0 && r.last >= total - 1)
        if (here != null && usable(here, failedAt)) {
            clusterNear(failedAt, here)?.let { return it }
        }
        if (firstClusterPos >= 0) {
            val target = (playheadMs() - JUMP_BACK_MS).coerceAtLeast(0)
            if (target > lastSampleMs + FAR_MS) {
                val estimate = estimateByte(target)
                val range = ranges.firstOrNull { estimate in it && usable(it, estimate) }
                    ?: ranges.firstOrNull { it.first > estimate && it.first - estimate < FAR_BYTES }
                if (range != null) {
                    clusterNear(maxOf(range.first, estimate), range)?.let { at ->
                        runStartMs = target
                        return at
                    }
                }
            }
        }
        waitForData()
        return null
    }

    /** The cluster containing [pos] (scanning back, not below [range]'s start), else the next one inside [range]. */
    private suspend fun clusterNear(pos: Long, range: LongRange): Long? =
        findClusterBackward(pos, floor = range.first) ?: findClusterForward(pos, minOf(RESYNC_SCAN_BYTES, range.last - pos + 1))

    private fun estimateByte(timeMs: Long): Long {
        val total = source.length
        if (total <= 0 || durationUs == C.TIME_UNSET || durationUs <= 0 || firstClusterPos < 0) return firstClusterPos.coerceAtLeast(0)
        val frac = (timeMs * 1000.0 / durationUs).coerceIn(0.0, 1.0)
        return firstClusterPos + ((total - firstClusterPos) * frac).toLong()
    }

    /** A cluster near [timeMs] (bitrate estimate) when that is clearly past [current]; null = stay. */
    private suspend fun jumpTo(timeMs: Long, current: Long): Long? {
        val estimate = estimateByte(timeMs)
        if (estimate <= firstClusterPos || estimate < current + MIN_JUMP_BYTES) return null
        val ranges = source.available()
        val range = ranges?.firstOrNull { estimate in it } ?: if (ranges != null) return null else 0L until Long.MAX_VALUE
        return clusterNear(estimate, range)
    }

    private suspend fun runMp4(container: Container) {
        var attempts = 0
        var lastAvailable = -1L
        while (true) {
            coroutineContext.ensureActive()
            val extractor = if (container == Container.FMP4) {
                FragmentedMp4Extractor(Media3SubtitleParser.Factory.UNSUPPORTED, FMP4_FLAGS)
            } else {
                Mp4Extractor(Media3SubtitleParser.Factory.UNSUPPORTED, MP4_FLAGS)
            }
            extractor.init(Output())
            val holder = PositionHolder()
            var input = input(0)
            try {
                while (true) {
                    coroutineContext.ensureActive()
                    when (extractor.read(input, holder)) {
                        Extractor.RESULT_SEEK -> input = input(holder.position)
                        Extractor.RESULT_END_OF_INPUT -> {
                            flushOpenCues()
                            return finish(complete = true)
                        }
                    }
                    publish()
                }
            } catch (e: DataNotAvailableException) {
                // MP4 cannot resync mid-file (the sample tables live in `moov`): retry from the start once more is downloaded.
                if (++attempts > MAX_MP4_RETRIES) return finish(failure = "not downloaded")
                resetPartials()
                while (true) {
                    waitForData()
                    val avail = source.available()?.sumOf { it.last - it.first + 1 } ?: Long.MAX_VALUE
                    if (avail - lastAvailable >= MP4_RETRY_GROWTH || avail == Long.MAX_VALUE) {
                        lastAvailable = avail
                        break
                    }
                }
            }
        }
    }

    // ---- resync -------------------------------------------------------------------------------------------------------------------

    private fun readable(pos: Long, len: Int): Int {
        val ranges = source.available() ?: return len
        val r = ranges.firstOrNull { pos in it } ?: return 0
        return min(len.toLong(), r.last - pos + 1).toInt()
    }

    private fun readBlock(pos: Long, buf: ByteArray, len: Int): Int {
        val n = readable(pos, len)
        if (n <= 0) return 0
        var got = 0
        while (got < n) {
            val r = source.readAt(pos + got, buf, got, n - got)
            if (r <= 0) break
            got += r
        }
        bytesRead += got
        if (bytesRead > policy.maxBytes) throw BudgetExceededException()
        return got
    }

    /** First Matroska Cluster at or after [from] within [maxScan] bytes (validated: size vint + a cluster child id). */
    private suspend fun findClusterForward(from: Long, maxScan: Long): Long? {
        val buf = ByteArray(SCAN_CHUNK + 16)
        var pos = from
        val end = from + maxScan
        while (pos < end) {
            coroutineContext.ensureActive()
            val n = readBlock(pos, buf, minOf(buf.size.toLong(), end - pos + 16).toInt())
            if (n < 4) return null
            // Full windows overlap by 16 bytes so a candidate near the edge is validated in the next one.
            val lastI = if (n == buf.size) n - 16 else n - 4
            for (i in 0..lastI) {
                if (isClusterAt(buf, i, n)) return pos + i
            }
            pos += lastI + 1
        }
        return null
    }

    /** Last Cluster starting at or before [pos] (scans back at most [BACK_SCAN_BYTES], never below [floor]). */
    private suspend fun findClusterBackward(pos: Long, floor: Long = 0): Long? {
        val buf = ByteArray(SCAN_CHUNK + 16)
        var windowEnd = pos + 4
        val stop = maxOf(pos - BACK_SCAN_BYTES, firstClusterPos.coerceAtLeast(0), floor)
        while (windowEnd > stop) {
            coroutineContext.ensureActive()
            val start = (windowEnd - SCAN_CHUNK).coerceAtLeast(stop)
            val n = readBlock(start, buf, (windowEnd - start).toInt() + 12)
            if (n < 4) return null
            for (i in minOf(n - 4, (pos - start).toInt()) downTo 0) {
                if (isClusterAt(buf, i, n)) return start + i
            }
            windowEnd = start + 3
            if (start == stop) break
        }
        return null
    }

    private fun isClusterAt(b: ByteArray, i: Int, n: Int): Boolean {
        if (b[i] != 0x1F.toByte() || b[i + 1] != 0x43.toByte() || b[i + 2] != 0xB6.toByte() || b[i + 3] != 0x75.toByte()) return false
        val sizeAt = i + 4
        if (sizeAt >= n) return true // cannot validate at the window edge; the extractor will
        val first = b[sizeAt].toInt() and 0xFF
        if (first == 0) return false
        val sizeLen = Integer.numberOfLeadingZeros(first) - 23
        val childAt = sizeAt + sizeLen
        if (childAt >= n) return true
        // Timestamp (E7) comes first in every muxer we know; Position (A7) / PrevSize (AB) are allowed before it.
        val child = b[childAt].toInt() and 0xFF
        return child == 0xE7 || child == 0xA7 || child == 0xAB
    }

    // ---- output -------------------------------------------------------------------------------------------------------------------

    private inner class Reader(private var position: Long) : DataReader, Closeable {
        private var raw: RawReader? = null

        override fun close() {
            runCatching { raw?.close() }
            raw = null
        }

        override fun read(buffer: ByteArray, offset: Int, length: Int): Int {
            if (length == 0) return 0
            val total = source.length
            if (total >= 0 && position >= total) return C.RESULT_END_OF_INPUT
            var n = length
            val ranges = source.available()
            if (ranges != null) {
                val r = ranges.firstOrNull { position in it } ?: throw DataNotAvailableException(position)
                n = min(n.toLong(), r.last - position + 1).toInt()
            }
            val reader = raw ?: source.openAt(position).also { raw = it }
            val got = reader.read(buffer, offset, n)
            if (got < 0) return C.RESULT_END_OF_INPUT
            position += got
            lastReaderPos = position
            bytesRead += got
            if (bytesRead > policy.maxBytes) throw BudgetExceededException()
            return got
        }
    }

    private inner class Output : ExtractorOutput {
        override fun track(id: Int, type: Int): TrackOutput {
            if (type != C.TRACK_TYPE_TEXT) return discard
            return collectors.getOrPut(id) { TextCollector(id, collectors.size) }
        }

        override fun endTracks() {
            tracksEnded = true
            publish(force = true)
        }

        override fun seekMap(seekMap: SeekMap) {
            durationUs = seekMap.durationUs
            // MatroskaExtractor (FLAG_DISABLE_SEEK_FOR_CUES) sends its map when the first Cluster starts: remember about where that is
            // (the reader is a few bytes past the cluster's id + size; only used for estimates and as the backward-scan floor).
            if (firstClusterPos < 0) firstClusterPos = (lastReaderPos - 16).coerceAtLeast(0)
        }
    }

    /** End of the last read of the extractor's input. */
    @Volatile
    private var lastReaderPos = 0L

    private inner class TextCollector(val id: Int, val ordinal: Int) : TrackOutput {
        var format: Format? = null
        private var buf = ByteArray(4096)
        private var size = 0
        private val seen = HashSet<Triple<Long, Long, String>>()
        val cues = ArrayList<TimedCue>()
        val skipped = HashSet<String>()
        private var open: SampleCue? = null
        var sorted = true

        val text get() = SubtitleSamples.isText(format?.sampleMimeType)

        override fun format(format: Format) {
            this.format = format
            publish(force = true)
        }

        override fun sampleData(input: DataReader, length: Int, allowEndOfInput: Boolean, sampleDataPart: Int): Int {
            if (!text) return discard.sampleData(input, length, allowEndOfInput, sampleDataPart)
            ensure(size + length)
            val n = input.read(buf, size, length)
            if (n == C.RESULT_END_OF_INPUT) {
                if (allowEndOfInput) return C.RESULT_END_OF_INPUT
                throw EOFException()
            }
            size += n
            return n
        }

        override fun sampleData(data: ParsableByteArray, length: Int, sampleDataPart: Int) {
            if (!text) {
                data.skipBytes(length)
                return
            }
            ensure(size + length)
            data.readBytes(buf, size, length)
            size += length
        }

        override fun sampleMetadata(timeUs: Long, flags: Int, size: Int, offset: Int, cryptoData: TrackOutput.CryptoData?) {
            if (!text) return
            val end = this.size - offset
            val start = end - size
            if (start >= 0 && end <= this.size) {
                val sample = buf.copyOfRange(start, end)
                runCatching { SubtitleSamples.decode(format?.sampleMimeType, timeUs / 1000, sample) }.getOrNull()?.let(::accept)
            }
            if (offset > 0 && end >= 0) System.arraycopy(buf, end, buf, 0, offset)
            this.size = offset.coerceAtMost(this.size)
        }

        private fun accept(c: SampleCue) {
            if (c.startMs > lastSampleMs) lastSampleMs = c.startMs
            if (c.endMs == null) {
                // tx3g / MP4 WebVTT: a sample lasts until the next one (an empty sample clears the screen).
                open?.let { prev -> add(prev.copy(endMs = maxOf(c.startMs, prev.startMs + 1))) }
                open = c.takeIf { it.text.isNotEmpty() }
                return
            }
            add(c)
        }

        fun flushOpen() {
            open?.let { add(it.copy(endMs = it.startMs + OPEN_CUE_MS)) }
            open = null
        }

        private fun add(c: SampleCue) {
            val end = c.endMs ?: return
            if (c.text.isEmpty() || end <= c.startMs) return
            if (c.karaoke || c.drawing) {
                skipped += c.text
                return
            }
            if (cues.size >= MAX_CUES_PER_TRACK) return
            // Layered typesetting repeats the same event (outline / shadow / glow layers): keep one.
            if (!seen.add(Triple(c.startMs, end, c.text))) return
            if (cues.isNotEmpty() && cues.last().startMs > c.startMs) sorted = false
            cues += TimedCue(c.startMs, end, c.text)
            dirtyCues++
        }

        fun resetPartial() {
            size = 0
            open = null
        }

        private fun ensure(n: Int) {
            if (n > buf.size) buf = buf.copyOf(maxOf(n, buf.size * 2))
        }
    }

    private fun resetPartials() = collectors.values.forEach { it.resetPartial() }

    private fun flushOpenCues() = collectors.values.forEach { it.flushOpen() }

    private fun publish(force: Boolean = false, waiting: Boolean = false) {
        val now = System.nanoTime() / 1_000_000
        if (!force && dirtyCues < PUBLISH_EVERY_CUES && now - lastPublish < PUBLISH_EVERY_MS) return
        lastPublish = now
        dirtyCues = 0
        _state.value = snapshot().copy(waiting = waiting)
    }

    private fun snapshot(): EmbeddedSubtitlesState {
        val tracks = collectors.values.mapNotNull { c ->
            val f = c.format ?: return@mapNotNull null
            EmbeddedTrack(c.id, c.ordinal, f.sampleMimeType, f.language, f.label)
        }
        val cues = collectors.values.filter { it.text }.associate { c ->
            if (!c.sorted) {
                c.cues.sortBy { it.startMs }
                c.sorted = true
            }
            c.id to c.cues.toList()
        }
        val skipped = collectors.values.filter { it.skipped.isNotEmpty() }.associate { it.id to it.skipped.toSet() }
        return EmbeddedSubtitlesState(tracks, cues, skipped)
    }

    private fun finish(complete: Boolean = false, failure: String? = null) {
        _state.value = snapshot().copy(complete = complete, failure = failure, finished = true)
    }

    companion object {
        /** Raw subtitle samples; no seeking to the Cues element at the end (a streaming torrent rarely has it yet). */
        private const val MKV_FLAGS = MatroskaExtractor.FLAG_EMIT_RAW_SUBTITLE_DATA or MatroskaExtractor.FLAG_DISABLE_SEEK_FOR_CUES
        private const val FMP4_FLAGS = FragmentedMp4Extractor.FLAG_EMIT_RAW_SUBTITLE_DATA
        private const val MP4_FLAGS = Mp4Extractor.FLAG_EMIT_RAW_SUBTITLE_DATA

        private const val WAIT_MS = 3_000L
        private const val MIN_RESUME_BYTES = 2L * 1024 * 1024
        private const val FAR_MS = 2 * 60_000L
        private const val JUMP_BACK_MS = 20_000L
        private const val FAR_BYTES = 64L * 1024 * 1024
        private const val MIN_JUMP_BYTES = 16L * 1024 * 1024
        private const val FAR_CHECK_MS = 10_000L
        private const val RESYNC_SCAN_BYTES = 16L * 1024 * 1024
        private const val BACK_SCAN_BYTES = 12L * 1024 * 1024
        private const val SCAN_CHUNK = 64 * 1024
        private const val MAX_RESYNCS = 32
        private const val MAX_MP4_RETRIES = 40
        private const val MP4_RETRY_GROWTH = 32L * 1024 * 1024
        private const val MAX_CUES_PER_TRACK = 20_000
        private const val OPEN_CUE_MS = 4_000L
        private const val PUBLISH_EVERY_CUES = 40
        private const val PUBLISH_EVERY_MS = 1_500L
    }
}
