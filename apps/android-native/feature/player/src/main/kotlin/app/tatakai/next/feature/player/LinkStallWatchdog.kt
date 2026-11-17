// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

/**
 * 0.4.1: a stuck PC session recovers by itself. The incident (2026-11-17): after a long pause the PC's engine stopped producing
 * without exiting, so every segment request waited 15 s and got `503 segment_wait_timeout` + Retry-After forever; between the
 * retries Media3 flips READY <-> BUFFERING (it goes READY when its renderers are ready and it is not loading), which flickered
 * "Tu PC está preparando el video…" for minutes. Nothing on the phone ever gave up or asked the PC for a fresh session.
 *
 * Pure and clock-driven (the player feeds it [observe] / events and asks [decide] about once a second):
 *
 * - **Stall**: the playback started, the user wants it to play (playing or buffering, not paused) and the position has not moved
 *   [PROGRESS_MS] forward for [stallMs] (9 s). Measured on the position, not on "buffering", because the flicker interrupts the
 *   buffering state without playing anything. Right after a seek the PC restarts its engine there (BUG-095: up to ~15 s with
 *   A4K + LSFG), so the first window after a seek (a jump of the position) is [seekStallMs] (25 s).
 * - **Terminal**: a media load the PC answered 404 / 410 (`session_not_found`, `session_gone`, `segment_unavailable`), or the
 *   session status said `failed` / `closed` ([onTerminal], [onStatus]).
 *
 * Either asks for a renewal of the session at the current position ([Action.Renew]; the player uses the existing renew path:
 * `source:{kind:"session"}`, same profile, `start_secs`), at most [backoffMs].size times: the first at once, the next ones at
 * least 5 s and 15 s after the previous one. [HEALTHY_MS] of position played without a stall gives the attempts back. With the
 * attempts spent it answers [Action.GiveUp] once (the player shows a real error with "Reproducir en el teléfono").
 *
 * **End of the episode** is never a failure: the PC's last segments answer 404 past the real end of the source (its container
 * duration may say a bit more), so a terminal error or a stall within [NEAR_END_MS] of the playlist's duration (older PCs), or
 * buffering / a terminal error within [PC_END_MS] of the `end_secs` the PC reports with `ended: true`, answers [Action.Ended]
 * (the player ends the playback like a local one: next-episode countdown, sleep timer...), never a renewal. Near the duration
 * a stall needs only [END_STALL_MS].
 */
internal class LinkStallWatchdog(
    private val stallMs: Long = STALL_MS,
    private val seekStallMs: Long = SEEK_STALL_MS,
    /** Minimum wait before attempt n+1, indexed by the attempts already made (index 0 = the first attempt). */
    private val backoffMs: List<Long> = BACKOFF_MS,
    private val healthyMs: Long = HEALTHY_MS,
) {
    enum class Reason { STALLED, TERMINAL_ERROR, SESSION_FAILED }

    sealed interface Action {
        data object Wait : Action

        /** Renew the session now; [attempt] is 1-based. */
        data class Renew(val reason: Reason, val attempt: Int) : Action

        /** Every attempt was spent: show the error (once, until [reset]). */
        data class GiveUp(val reason: Reason) : Action

        /** The episode reached its end: finish the playback normally (once, until a seek or a new load). */
        data object Ended : Action
    }

    var attempts = 0
        private set

    private var nextAllowedAt = 0L
    private var gaveUp = false

    // stall measurement
    private var anchorPos = 0L
    private var anchorAt = 0L
    private var measuring = false
    private var windowMs = stallMs

    /** Position where the current healthy stretch started; null while stalled / right after a renewal. */
    private var healthyFrom: Long? = null

    private var terminal: Reason? = null

    // end of the episode
    private var durationMs = 0L
    private var buffering = false
    private var pcEndMs: Long? = null
    private var finished = false

    /** A new load of the Link session (first load, renewal, profile change): idle until it starts, the attempts are kept. */
    fun onLoad() {
        measuring = false
        terminal = null
        windowMs = stallMs
        healthyFrom = null
        last = null
        finished = false
    }

    /** A media load the PC answered "this session / segment is gone for good". */
    fun onTerminal() {
        terminal = Reason.TERMINAL_ERROR
    }

    /**
     * `GET /v1/sessions/{id}`: `failed` / `closed` cannot recover by waiting; `ended` + `end_secs` (0.4.1 PCs) is where the
     * source really ends.
     */
    fun onStatus(state: String?, ended: Boolean = false, endSecs: Double? = null) {
        if (state == "failed" || state == "closed") terminal = Reason.SESSION_FAILED
        if (ended && endSecs != null && endSecs > 0) pcEndMs = (endSecs * 1000).toLong()
    }

    /**
     * One look at the engine. [wantsPlay] = playing or buffering (a user pause is neither and never counts as a stall).
     */
    fun observe(now: Long, started: Boolean, wantsPlay: Boolean, positionMs: Long, durationMs: Long = 0, buffering: Boolean = false) {
        this.durationMs = durationMs
        this.buffering = started && buffering
        // A seek (the user's, or skip intro) shows as a jump the playback itself cannot make: back, or forward faster than 2x.
        // The PC restarts its engine there (BUG-095), so the next window is the longer one.
        val jumped = last != null && (positionMs < lastPos - PROGRESS_MS || positionMs > lastPos + (if (lastPlaying) 2 * (now - lastAt) else 0) + SEEK_JUMP_MS)
        last = now
        lastPos = positionMs
        lastAt = now
        lastPlaying = started && wantsPlay
        if (jumped) {
            windowMs = seekStallMs
            healthyFrom = null
            finished = false // a seek back after the end plays again
        }
        if (!started || !wantsPlay) {
            measuring = false
            return
        }
        if (!measuring || jumped) {
            measuring = true
            anchorPos = positionMs
            anchorAt = now
            return
        }
        if (positionMs >= anchorPos + PROGRESS_MS) {
            anchorPos = positionMs
            anchorAt = now
            windowMs = stallMs
            val from = healthyFrom ?: positionMs.also { healthyFrom = it }
            if (positionMs - from >= healthyMs) {
                attempts = 0
                nextAllowedAt = 0
            }
        } else if (now - anchorAt > PROGRESS_MS * 3) {
            healthyFrom = null // a stretch without progress is not healthy playback
        }
    }

    private var last: Long? = null
    private var lastPos = 0L
    private var lastAt = 0L
    private var lastPlaying = false

    /** Within [NEAR_END_MS] of the playlist's duration (any PC). */
    private fun nearDurationEnd(): Boolean = durationMs > 0 && lastPos >= durationMs - NEAR_END_MS

    /** Within [PC_END_MS] of where the PC said the source ends. */
    private fun nearPcEnd(): Boolean = pcEndMs?.let { lastPos >= it - PC_END_MS } == true

    fun decide(now: Long): Action {
        if (finished) return Action.Wait
        if (nearPcEnd() && (buffering || terminal != null)) return ended()
        val window = if (nearDurationEnd()) minOf(windowMs, END_STALL_MS) else windowMs
        val stalled = measuring && now - anchorAt >= window
        if ((terminal != null || stalled) && (nearDurationEnd() || nearPcEnd())) return ended()
        if (gaveUp) return Action.Wait
        val reason = terminal ?: if (stalled) Reason.STALLED else return Action.Wait
        if (attempts >= backoffMs.size) {
            gaveUp = true
            return Action.GiveUp(reason)
        }
        if (now < nextAllowedAt) return Action.Wait
        return Action.Renew(reason, attempts + 1)
    }

    private fun ended(): Action {
        finished = true
        terminal = null
        return Action.Ended
    }

    /** The player started the renewal [decide] asked for. */
    fun onRenewing(now: Long) {
        attempts++
        nextAllowedAt = now + backoffMs.getOrElse(attempts) { 0L }
        onLoad()
    }

    /** The user's Retry / another location: everything starts over. */
    fun reset() {
        attempts = 0
        nextAllowedAt = 0
        gaveUp = false
        pcEndMs = null
        onLoad()
    }

    companion object {
        const val STALL_MS = 9_000L
        const val SEEK_STALL_MS = 25_000L
        val BACKOFF_MS = listOf(0L, 5_000L, 15_000L)
        const val HEALTHY_MS = 60_000L

        /** The position must move this much to count as playing (READY between two failed loads moves it a few ms). */
        const val PROGRESS_MS = 1_000L

        /** A forward jump past what 2x playback explains by more than this is a seek. */
        const val SEEK_JUMP_MS = 3_000L

        /** Older PCs: a dead end this close to the playlist's duration is the end of the episode. */
        const val NEAR_END_MS = 6_000L

        /** 0.4.1 PCs: this close to their `end_secs`, buffering is the end of the episode. */
        const val PC_END_MS = 4_000L

        /** Near the end a stall is the end after this long (not the 9 s of a stuck engine). */
        const val END_STALL_MS = 3_000L
    }
}

/**
 * 0.4.1: the Link buffering card ("Tu PC está preparando el video…") must not blink. Media3 flips READY <-> BUFFERING between
 * retries; the card appears only after [showAfterMs] of continuous buffering and goes away only after [hideAfterMs] without it.
 * Pure: [update] with the raw state returns what to show, [nextChangeAt] says when to look again with nothing new.
 */
internal class BufferingDebounce(
    private val showAfterMs: Long = SHOW_AFTER_MS,
    private val hideAfterMs: Long = HIDE_AFTER_MS,
) {
    var shown = false
        private set

    private var raw = false
    private var rawSince = 0L

    /** A new load: nothing shown (the loading phase has its own card), [buffering] from now. */
    fun reset(now: Long, buffering: Boolean = false) {
        shown = false
        raw = buffering
        rawSince = now
    }

    fun update(buffering: Boolean, now: Long): Boolean {
        if (buffering != raw) {
            raw = buffering
            rawSince = now
        }
        if (raw != shown && now - rawSince >= if (raw) showAfterMs else hideAfterMs) shown = raw
        return shown
    }

    /** When [update] could change [shown] without a new raw state; null = stable. */
    fun nextChangeAt(): Long? = if (raw == shown) null else rawSince + if (raw) showAfterMs else hideAfterMs

    companion object {
        const val SHOW_AFTER_MS = 800L
        const val HIDE_AFTER_MS = 1_000L
    }
}
