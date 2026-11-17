// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.model.LinkProcessingInfo
import app.tatakai.next.player.rangeSpans
import app.tatakai.next.player.readySpan

// 0.4.1 (user report 2026-11-17, "como en PC"): the seek bar tells apart what the phone already has and what the PC already
// processed, each in its own colour. Pure, so the mapping is unit tested; the drawing lives in PlayerControls' SeekBar.

/** One coloured run of the seek bar: [left] / [width] are fractions (0..1) of the duration. */
data class BarSpan(val left: Float, val width: Float) {
    val right: Float get() = left + width
}

/**
 * The runs drawn over the part not played yet:
 * - [buffered]: what the engine has ahead of the playhead (Media3 `bufferedPosition`, mpv `demuxer-cache-time`; for a torrent the
 *   downloaded run that starts at the playhead) — "Descargado";
 * - [downloaded]: a torrent's downloaded pieces anywhere in the file (the 0.3 dashes) — "Descargado" too, dimmer;
 * - [pcProcessed]: Tatakai Link (PC / Mixto): the source range the PC already processed — "Procesado por la PC";
 * - [pcEstimated]: [pcProcessed] comes from the PC's lead (an older PC without the range), not from the PC's own range.
 */
data class SeekBarRanges(
    val buffered: BarSpan? = null,
    val downloaded: List<BarSpan> = emptyList(),
    val pcProcessed: BarSpan? = null,
    val pcEstimated: Boolean = false,
) {
    /** The legend under the bar: only when there is more than the plain buffer every player has (a torrent, the PC). */
    val showLegend: Boolean get() = pcProcessed != null || downloaded.isNotEmpty()

    /** "Descargado" has something to point at. */
    val hasDownloaded: Boolean get() = buffered != null || downloaded.isNotEmpty()

    companion object {
        val NONE = SeekBarRanges()
    }
}

/**
 * Maps the engine / torrent / PC facts to the seek bar's runs. [torrentMarks] are the torrent's downloaded byte ranges of the
 * playing file (fractions, `TorrentRepository.seekMarks`), [pc] what the PC does (null when the phone plays the source itself).
 * The PC's range is `processedFromSecs..processedToSecs` when the PC says it, else estimated as the playhead + `leadSecs`.
 */
fun seekBarRanges(
    positionMs: Long,
    durationMs: Long,
    bufferedMs: Long,
    torrentMarks: List<ClosedFloatingPointRange<Double>> = emptyList(),
    pc: LinkProcessingInfo? = null,
): SeekBarRanges {
    if (durationMs <= 0) return SeekBarRanges.NONE
    val dur = durationMs.toDouble()
    val at = (positionMs.coerceAtLeast(0) / dur).coerceIn(0.0, 1.0)
    val downloaded = rangeSpans(torrentMarks).map { (l, w) -> BarSpan(l, w) }
    val buffered = if (torrentMarks.isNotEmpty()) {
        readySpan(torrentMarks, at)?.let { (l, w) -> BarSpan(l, w) }
    } else {
        span(at, bufferedMs / dur)
    }
    var estimated = false
    val processed = pc?.let { info ->
        val from = info.processedFromSecs
        val to = info.processedToSecs
        if (from != null && to != null && from.isFinite() && to.isFinite()) {
            span(from * 1000 / dur, to * 1000 / dur)
        } else {
            info.leadSecs?.takeIf { it.isFinite() && it > 0 }?.let { lead ->
                estimated = true
                span(at, (positionMs + lead * 1000) / dur)
            }
        }
    }
    return SeekBarRanges(buffered = buffered, downloaded = downloaded, pcProcessed = processed, pcEstimated = processed != null && estimated)
}

/** `from..to` (fractions) clamped to the bar; null when empty. */
private fun span(from: Double, to: Double): BarSpan? {
    if (!from.isFinite() || !to.isFinite()) return null
    val a = from.coerceIn(0.0, 1.0)
    val b = to.coerceIn(0.0, 1.0)
    // Less than 0.1 % of the bar is not a run worth drawing (the buffer right at the playhead).
    return if (b - a > MIN_SPAN) BarSpan(a.toFloat(), (b - a).toFloat()) else null
}

private const val MIN_SPAN = 0.001
