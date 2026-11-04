// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.SkipKind
import app.tatakai.next.model.SkipMark

// android wave (docs/redesign/12 §4 d.3): intro / outro / recap markers (AniSkip) and torrent seek-bar ranges, ported 1:1 from desktop
// apps/desktop-native/crates/tk-ui/src/player_state.rs (Markers, range_spans, downloaded_fraction, ready_span). Pure.

/** A marked range in seconds; [contains] is start-inclusive, end-exclusive. */
data class Span(val start: Double, val end: Double) {
    fun contains(pos: Double): Boolean = pos >= start && pos < end
}

/** One marker on the seek bar as fractions of the bar: [left] and [width] in 0..1. */
data class BarMark(val kind: SkipKind, val left: Float, val width: Float)

data class Markers(val intro: Span? = null, val outro: Span? = null, val recap: Span? = null) {
    private fun spans(): List<Pair<SkipKind, Span?>> = listOf(SkipKind.RECAP to recap, SkipKind.INTRO to intro, SkipKind.OUTRO to outro)

    val isEmpty: Boolean get() = intro == null && outro == null && recap == null

    /** The mark [posSecs] is inside and where skipping it jumps to (its end). */
    fun skipAt(posSecs: Double): Pair<SkipKind, Double>? =
        spans().firstNotNullOfOrNull { (kind, s) -> s?.takeIf { it.contains(posSecs) }?.let { kind to it.end } }

    /** Where "Skip intro" jumps to, while [posSecs] is inside the intro. */
    fun skipTarget(posSecs: Double): Double? = intro?.takeIf { it.contains(posSecs) }?.end

    /** Every marker with its kind, in recap / intro / outro order, as fractions of a bar of [durationSecs]. */
    fun barMarks(durationSecs: Double): List<BarMark> {
        if (!(durationSecs.isFinite() && durationSecs > 0.0)) return emptyList()
        return spans().mapNotNull { (kind, s) -> s?.let { kind to it } }
            .filter { (_, s) -> s.end > s.start && s.start < durationSecs }
            .map { (kind, s) ->
                val a = (s.start / durationSecs).coerceIn(0.0, 1.0)
                val b = (s.end / durationSecs).coerceIn(0.0, 1.0)
                BarMark(kind, a.toFloat(), (b - a).toFloat())
            }
    }

    companion object {
        /** The first mark of each kind (AniSkip may list more than one; the earliest wins). Malformed marks are skipped. */
        fun from(marks: List<SkipMark>): Markers {
            var out = Markers()
            for (m in marks) {
                if (!(m.startSecs.isFinite() && m.endSecs.isFinite() && m.endSecs > m.startSecs)) continue
                val span = Span(m.startSecs.coerceAtLeast(0.0), m.endSecs)
                out = when (m.kind) {
                    SkipKind.INTRO -> if (out.intro == null || span.start < out.intro!!.start) out.copy(intro = span) else out
                    SkipKind.OUTRO -> if (out.outro == null || span.start < out.outro!!.start) out.copy(outro = span) else out
                    SkipKind.RECAP -> if (out.recap == null || span.start < out.recap!!.start) out.copy(recap = span) else out
                }
            }
            return out
        }
    }
}

/**
 * Downloaded ranges of the playing file (`TorrentRepository.seekMarks`, byte fractions 0..1) as `(left, width)` fractions of the seek bar.
 * Byte fractions stand in for time fractions (near-constant bitrate).
 */
fun rangeSpans(ranges: List<ClosedFloatingPointRange<Double>>): List<Pair<Float, Float>> =
    ranges.asSequence()
        .filter { it.start.isFinite() && it.endInclusive.isFinite() }
        .map { it.start.coerceIn(0.0, 1.0) to it.endInclusive.coerceIn(0.0, 1.0) }
        .filter { (a, b) -> b > a }
        .map { (a, b) -> a.toFloat() to (b - a).toFloat() }
        .toList()

/** Share of the file already downloaded (sum of the ranges), 0..1. */
fun downloadedFraction(ranges: List<ClosedFloatingPointRange<Double>>): Double =
    ranges.asSequence()
        .filter { it.start.isFinite() && it.endInclusive.isFinite() && it.endInclusive > it.start }
        .sumOf { (it.endInclusive.coerceAtMost(1.0) - it.start.coerceAtLeast(0.0)).coerceAtLeast(0.0) }
        .coerceIn(0.0, 1.0)

/**
 * "Ready to play" from [at] (fraction of the file): `(left, width)` from [at] to the end of the downloaded range containing it; null when
 * the position is not downloaded yet (the engine waits for data there).
 */
fun readySpan(ranges: List<ClosedFloatingPointRange<Double>>, at: Double): Pair<Float, Float>? {
    if (!at.isFinite()) return null
    val p = at.coerceIn(0.0, 1.0)
    // A tiny slack: right after a seek the playhead sits on a range boundary.
    val r = ranges.firstOrNull { p >= it.start - 1e-6 && p < it.endInclusive } ?: return null
    val w = (r.endInclusive.coerceAtMost(1.0) - p).coerceAtLeast(0.0)
    return if (w > 0.0) p.toFloat() to w.toFloat() else null
}

/** A torrent id / v1 info-hash: 40 hex characters. */
fun isInfoHash(s: String): Boolean = s.length == 40 && s.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }

/** The v1 info-hash of a magnet (`xt=urn:btih:<40 hex>`), lowercased; null for base32 hashes or anything else. */
fun magnetInfoHash(uri: String): String? {
    if (isInfoHash(uri)) return uri.lowercase()
    val m = Regex("xt=urn:btih:([0-9a-fA-F]{40})").find(uri) ?: return null
    return m.groupValues[1].lowercase()
}

/** Seconds left on the next-episode card that appeared at [sinceMs]. */
fun countdownLeft(sinceMs: Long, nowMs: Long, totalSecs: Int = NEXT_COUNTDOWN_SECS): Int =
    (totalSecs - ((nowMs - sinceMs).coerceAtLeast(0) / 1000).toInt()).coerceAtLeast(0)

/** Next-episode card countdown (prototype `player.js`). */
const val NEXT_COUNTDOWN_SECS: Int = 10
