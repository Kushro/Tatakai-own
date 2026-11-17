// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.LinkFrameGen
import app.tatakai.next.model.LinkProcessingInfo
import app.tatakai.next.model.LinkUpscale
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 0.4.1: the seek bar's coloured runs (engine buffer, torrent pieces, what the PC already processed). */
class SeekBarRangesTest {
    private val eps = 1e-4f

    private fun pc(lead: Double? = null, from: Double? = null, to: Double? = null) = LinkProcessingInfo(
        pcName = "PC", profileId = "p", profileLabel = null, upscale = LinkUpscale.None, outputWidth = null, outputHeight = null,
        frameGen = LinkFrameGen.None, encoder = null, videoKbps = null, leadSecs = lead, processedFromSecs = from, processedToSecs = to,
    )

    private fun assertSpan(left: Float, width: Float, span: BarSpan?) {
        requireNotNull(span)
        assertEquals(left, span.left, eps)
        assertEquals(width, span.width, eps)
    }

    @Test
    fun nothingWithoutADuration() {
        assertEquals(SeekBarRanges.NONE, seekBarRanges(positionMs = 10_000, durationMs = 0, bufferedMs = 50_000))
    }

    @Test
    fun engineBufferFromThePlayhead() {
        // 100 s video at 20 s, buffered to 50 s
        val r = seekBarRanges(20_000, 100_000, 50_000)
        assertSpan(0.2f, 0.3f, r.buffered)
        assertTrue(r.downloaded.isEmpty())
        assertNull(r.pcProcessed)
        assertTrue(r.hasDownloaded)
        assertFalse(r.showLegend, "a plain buffer needs no legend")
    }

    @Test
    fun noBufferAheadDrawsNothing() {
        assertNull(seekBarRanges(20_000, 100_000, 20_000).buffered)
        assertNull(seekBarRanges(20_000, 100_000, 5_000).buffered, "a buffer behind the playhead is not ahead of it")
        assertSpan(0.9f, 0.1f, seekBarRanges(90_000, 100_000, 400_000).buffered) // clamped to the end
    }

    @Test
    fun torrentPiecesAndTheReadyRun() {
        val marks = listOf(0.0..0.4, 0.6..0.8)
        val r = seekBarRanges(positionMs = 30_000, durationMs = 100_000, bufferedMs = 90_000, torrentMarks = marks)
        assertEquals(2, r.downloaded.size)
        assertSpan(0.6f, 0.2f, r.downloaded[1])
        // the torrent's own ready run wins over the engine's buffer position
        assertSpan(0.3f, 0.1f, r.buffered)
        assertTrue(r.showLegend)
        // playhead in a hole: nothing ready there
        assertNull(seekBarRanges(50_000, 100_000, 90_000, marks).buffered)
    }

    @Test
    fun thePcsOwnProcessedRange() {
        // 24 min episode at 2 min; the PC processed 1:00..6:00
        val r = seekBarRanges(120_000, 1_440_000, 150_000, pc = pc(lead = 30.0, from = 60.0, to = 360.0))
        assertSpan(60f / 1440f, 300f / 1440f, r.pcProcessed)
        assertFalse(r.pcEstimated)
        assertTrue(r.showLegend)
    }

    @Test
    fun anOlderPcIsEstimatedFromItsLead() {
        val r = seekBarRanges(120_000, 1_440_000, 150_000, pc = pc(lead = 48.0))
        assertSpan(120f / 1440f, 48f / 1440f, r.pcProcessed)
        assertTrue(r.pcEstimated)
        // neither the range nor a lead: nothing to draw (and no legend for it)
        val none = seekBarRanges(120_000, 1_440_000, 150_000, pc = pc())
        assertNull(none.pcProcessed)
        assertFalse(none.showLegend)
        // half a range is not a range: the lead is used
        assertTrue(seekBarRanges(120_000, 1_440_000, 0, pc = pc(lead = 10.0, from = 60.0)).pcEstimated)
    }

    @Test
    fun aRangePastTheEndIsClamped() {
        val r = seekBarRanges(0, 100_000, 0, pc = pc(from = 90.0, to = 130.0))
        assertSpan(0.9f, 0.1f, r.pcProcessed)
        assertNull(seekBarRanges(0, 100_000, 0, pc = pc(from = 50.0, to = 40.0)).pcProcessed, "an inverted range is empty")
    }
}
