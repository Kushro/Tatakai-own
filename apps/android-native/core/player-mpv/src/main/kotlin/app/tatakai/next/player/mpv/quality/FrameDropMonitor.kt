// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv.quality

// 0.4.0 (worker A), replaces BUG-043's PresetGovernor sampling: mpv's cumulative drop counters (`frame-drop-count` +
// `decoder-frame-drop-count`) are sampled while playing; a window of at least [windowMs] whose drops exceed [maxRatio] of the frames
// it should have shown (and at least [minDrops], so a 2-frame hiccup on a 24 fps video never counts) is "too slow". Enigmata uses
// ">45 drops in 10 s" (`ProfileEngine.shouldAutoFallback`); a ratio works the same for 24 and 60 fps sources. Pure, unit tested.
class FrameDropMonitor(
    private val windowMs: Long = WINDOW_MS,
    private val maxRatio: Double = MAX_RATIO,
    private val minDrops: Long = MIN_DROPS,
) {
    private var startDrops: Long? = null
    private var startMs = 0L

    /** A seek / pause / buffering / new file / quality change: drops across it are not the chain's fault. */
    fun reset() {
        startDrops = null
    }

    /**
     * One sample while playing. [cumulativeDrops] = the counters since the file started, [fps] = the video frame rate, [nowMs] a monotonic
     * clock. True when the window just closed was too slow (the monitor then starts a fresh window).
     */
    fun sample(cumulativeDrops: Long, fps: Double, nowMs: Long): Boolean {
        val start = startDrops
        if (start == null || cumulativeDrops < start || fps <= 0.0) {
            startDrops = cumulativeDrops
            startMs = nowMs
            return false
        }
        val elapsed = nowMs - startMs
        if (elapsed < windowMs) return false
        val dropped = cumulativeDrops - start
        val expected = fps * elapsed / 1000.0
        startDrops = cumulativeDrops
        startMs = nowMs
        return dropped >= minDrops && dropped / expected > maxRatio
    }

    companion object {
        const val WINDOW_MS = 8_000L
        const val MAX_RATIO = 0.15
        const val MIN_DROPS = 12L
        const val SAMPLE_MS = 2_000L
    }
}
