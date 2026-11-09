// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToLong

// parity wave A (docs/redesign/15 §3.2): sleep timer (player.sleep-timer) and subtitle delay (player.subs.offset), ported from the
// classic `usePlayerEnhancements.ts` / `VideoSettingsPanel.tsx`. Owner: worker p. Pure, no Android, no clock of their own.

/** Sleep timer options of the player menu. [minutes] = null for "end of the episode". */
enum class SleepOption(val id: String, val minutes: Int?) {
    OFF("off", null),
    MIN_15("15", 15),
    MIN_30("30", 30),
    MIN_60("60", 60),
    END_OF_EPISODE("end", null),
}

/**
 * When the sleep timer fires: after [SleepOption.minutes] of wall time, or when the episode ends ([SleepOption.END_OF_EPISODE]). When it
 * fires the player pauses and does NOT move on to the next episode (the countdown card is cancelled). [nowMs] is an elapsed-realtime
 * clock supplied by the caller.
 */
class SleepTimer {
    var option: SleepOption = SleepOption.OFF
        private set
    private var deadlineMs: Long? = null

    /** True once the timer fired; stays true until [set] is called again, so the player keeps "next episode" off. */
    var fired: Boolean = false
        private set

    fun set(option: SleepOption, nowMs: Long) {
        this.option = option
        fired = false
        deadlineMs = option.minutes?.let { nowMs + it * 60_000L }
    }

    /** Milliseconds left, null when there is no wall-clock deadline (off, end of episode, or already fired). */
    fun remainingMs(nowMs: Long): Long? = deadlineMs?.takeIf { !fired }?.let { (it - nowMs).coerceAtLeast(0) }

    /** Call on every tick: true exactly once, when the deadline passes. */
    fun tick(nowMs: Long): Boolean {
        val d = deadlineMs ?: return false
        if (fired || nowMs < d) return false
        fire()
        return true
    }

    /** Call when the episode ended: true when the "end of episode" timer fires now. */
    fun onEpisodeEnded(): Boolean {
        if (fired || option != SleepOption.END_OF_EPISODE) return false
        fire()
        return true
    }

    /** A timer is armed (not off and not fired). */
    val active: Boolean get() = option != SleepOption.OFF && !fired

    private fun fire() {
        fired = true
        deadlineMs = null
    }
}

/** Subtitle delay (mpv `sub-delay`; Media3: cues shifted by the screen): steps of 100 ms, clamped to ±60 s. */
object SubtitleDelay {
    const val STEP_MS: Long = 100
    const val MAX_MS: Long = 60_000

    /** [currentMs] moved one step in [direction] (+1 later, -1 earlier). */
    fun step(currentMs: Long, direction: Int): Long = clamp(currentMs + STEP_MS * direction.coerceIn(-1, 1))

    fun clamp(ms: Long): Long = ms.coerceIn(-MAX_MS, MAX_MS)

    /** mpv wants seconds. */
    fun seconds(ms: Long): Double = clamp(ms) / 1000.0

    /** "+0.3 s", "-1.2 s", "0.0 s" (tenths, dot separator: the same text in every locale, like the classic panel). */
    fun format(ms: Long): String {
        val tenths = (clamp(ms) / 100.0).roundToLong()
        val sign = when {
            tenths > 0 -> "+"
            tenths < 0 -> "-"
            else -> ""
        }
        val a = abs(tenths)
        return String.format(Locale.ROOT, "%s%d.%d s", sign, a / 10, a % 10)
    }

    /**
     * Media3 cues are drawn by the screen, so the delay is applied by asking which cue is visible at `position - delay`: a positive delay
     * shows a subtitle later. Returns the position to look up.
     */
    fun lookupPositionMs(positionMs: Long, delayMs: Long): Long = (positionMs - clamp(delayMs)).coerceAtLeast(0)
}
