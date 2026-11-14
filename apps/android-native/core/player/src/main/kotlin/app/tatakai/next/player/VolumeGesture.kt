// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlin.math.abs

/**
 * BUG-044: one vertical volume drag of the player gesture layer. It starts from the system volume read when the finger goes down
 * (never from a value cached when the player opened), never touches a muted stream, and moves one step per `FULL_RANGE_TRAVEL / max` of the
 * screen height so a short swipe cannot jump from 0 to the maximum. Pure, so the arithmetic is unit tested.
 *
 * @param startIndex the `AudioManager.getStreamVolume(STREAM_MUSIC)` value at drag start.
 * @param maxIndex the stream's maximum index.
 * @param muted `AudioManager.isStreamMute(STREAM_MUSIC)` at drag start: a muted stream stays muted (the HUD says so instead).
 */
class VolumeDrag(startIndex: Int, maxIndex: Int, val muted: Boolean) {
    val max: Int = maxIndex.coerceAtLeast(1)
    private val start: Int = startIndex.coerceIn(0, max)
    private var travel = 0f

    /** The index the stream is at (as far as this drag knows). */
    var index: Int = start
        private set

    val percent: Int get() = if (muted) 0 else index * 100 / max

    /**
     * Adds [fraction] of the screen height (positive = finger moved up). Returns the new index to apply, or null when nothing has to
     * change (same step, or muted).
     */
    fun onDrag(fraction: Float): Int? {
        if (muted) return null
        travel += fraction
        // A full-range sweep needs FULL_RANGE_TRAVEL of the height; steps are truncated toward the start, so the first step needs a real
        // movement and a tiny jitter never rounds up.
        val steps = (travel / FULL_RANGE_TRAVEL * max).let { if (abs(it) < 1f) 0 else it.toInt() }
        val next = (start + steps).coerceIn(0, max)
        if (next == index) return null
        index = next
        return next
    }

    companion object {
        /** Fraction of the screen height that sweeps the whole range. */
        const val FULL_RANGE_TRAVEL = 1.2f
    }
}
