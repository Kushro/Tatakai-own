// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.feature.player.LinkStallWatchdog.Action
import app.tatakai.next.feature.player.LinkStallWatchdog.Reason
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 0.4.1: a stuck PC session (the 2026-11-17 pause incident) is renewed by itself, a few times, then the phone is offered. */
class LinkStallWatchdogTest {
    private var now = 1_000_000L
    private var pos = 640_000L
    private val w = LinkStallWatchdog()

    /** [secs] seconds of playback (position advancing) or of a stall (position still), observed every 500 ms like the engine tick. */
    private fun run(secs: Double, playing: Boolean, wantsPlay: Boolean = true, started: Boolean = true) {
        repeat((secs * 2).toInt()) {
            now += 500
            if (playing) pos += 500
            w.observe(now, started, wantsPlay, pos)
        }
    }

    @Test
    fun playbackThatKeepsMovingIsNeverRenewed() {
        run(120.0, playing = true)
        assertEquals(Action.Wait, w.decide(now))
    }

    @Test
    fun nineSecondsWithoutProgressRenewsAtOnce() {
        run(5.0, playing = true)
        // the last progress was seen up to a tick before the stall began
        run(8.0, playing = false)
        assertEquals(Action.Wait, w.decide(now))
        run(1.0, playing = false)
        assertEquals(Action.Renew(Reason.STALLED, 1), w.decide(now))
    }

    @Test
    fun theReadyBufferingFlickerStillCountsAsAStall() {
        // Media3 goes READY between two failed loads and the position creeps a few ms: not playback
        run(5.0, playing = true)
        repeat(20) {
            now += 500
            pos += 20
            w.observe(now, true, true, pos)
        }
        assertEquals(Reason.STALLED, (w.decide(now) as Action.Renew).reason)
    }

    @Test
    fun aUserPauseIsNotAStall() {
        run(5.0, playing = true)
        run(300.0, playing = false, wantsPlay = false)
        assertEquals(Action.Wait, w.decide(now))
        // resuming starts a fresh window
        run(3.0, playing = false)
        assertEquals(Action.Wait, w.decide(now))
    }

    @Test
    fun beforeTheFirstFrameTheWatchdogWaits() {
        run(60.0, playing = false, started = false)
        assertEquals(Action.Wait, w.decide(now))
    }

    @Test
    fun aSeekGetsTheLongerWindowForThePcRestart() {
        run(5.0, playing = true)
        pos += 300_000 // seek 5 min ahead: the PC restarts its engine there (~15 s with A4K + LSFG)
        run(15.0, playing = false)
        assertEquals(Action.Wait, w.decide(now))
        run(11.0, playing = false)
        assertEquals(Action.Renew(Reason.STALLED, 1), w.decide(now))
    }

    @Test
    fun renewalsBackOffAndStopAfterThree() {
        run(5.0, playing = true)
        run(9.0, playing = false)
        assertEquals(Action.Renew(Reason.STALLED, 1), w.decide(now))
        w.onRenewing(now)
        // the renewed session fails at once: the second try waits 5 s after the first
        w.onTerminal()
        assertEquals(Action.Wait, w.decide(now + 4_000))
        now += 5_000
        assertEquals(Action.Renew(Reason.TERMINAL_ERROR, 2), w.decide(now))
        w.onRenewing(now)
        w.onTerminal()
        assertEquals(Action.Wait, w.decide(now + 14_000))
        now += 15_000
        assertEquals(Action.Renew(Reason.TERMINAL_ERROR, 3), w.decide(now))
        w.onRenewing(now)
        w.onTerminal()
        now += 60_000
        assertEquals(Action.GiveUp(Reason.TERMINAL_ERROR), w.decide(now))
        // once: the error screen stays until the user acts
        assertEquals(Action.Wait, w.decide(now))
        w.reset()
        w.onTerminal()
        assertEquals(Action.Renew(Reason.TERMINAL_ERROR, 1), w.decide(now))
    }

    @Test
    fun aMinuteOfHealthyPlaybackGivesTheAttemptsBack() {
        w.onTerminal()
        w.onRenewing(now)
        w.onTerminal()
        now += 5_000
        w.onRenewing(now)
        assertEquals(2, w.attempts)
        w.onLoad()
        run(30.0, playing = true)
        assertEquals(2, w.attempts)
        run(33.0, playing = true)
        assertEquals(0, w.attempts)
    }

    @Test
    fun aFailedOrClosedSessionStatusRenewsWithoutWaitingForAStall() {
        run(5.0, playing = true)
        w.onStatus("ready")
        w.onStatus("preparing")
        assertEquals(Action.Wait, w.decide(now))
        w.onStatus("failed")
        assertEquals(Action.Renew(Reason.SESSION_FAILED, 1), w.decide(now))
        w.onRenewing(now)
        w.onStatus("closed")
        now += 5_000
        assertEquals(Action.Renew(Reason.SESSION_FAILED, 2), w.decide(now))
    }

    // ---- end of the episode: never a failure, never a renewal ----

    private fun observeAt(position: Long, duration: Long, buffering: Boolean, started: Boolean = true) {
        now += 500
        pos = position
        w.observe(now, started, true, pos, duration, buffering)
    }

    @Test
    fun aTerminalErrorAtTheEndOfAnOldPcsPlaylistEndsTheEpisode() {
        // an older PC: the container said 1440 s, the last segments answer 404 at 1436 s
        observeAt(1_436_000, 1_440_000, buffering = true)
        w.onTerminal()
        assertEquals(Action.Ended, w.decide(now))
        // once, and no renewal was counted
        assertEquals(Action.Wait, w.decide(now + 60_000))
        assertEquals(0, w.attempts)
    }

    @Test
    fun aStallNearTheDurationEndsTheEpisodeQuickly() {
        repeat(4) { observeAt(1_433_000L + it * 500, 1_440_000, buffering = false) }
        repeat(8) { observeAt(1_435_000, 1_440_000, buffering = true) }
        assertEquals(Action.Ended, w.decide(now))
    }

    @Test
    fun theSameFailureFarFromTheEndIsADeadSessionInstead() {
        observeAt(700_000, 1_440_000, buffering = true)
        w.onTerminal()
        assertEquals(Action.Renew(Reason.TERMINAL_ERROR, 1), w.decide(now))
    }

    @Test
    fun aNewPcSaysWhereTheSourceEndsAndBufferingThereIsTheEnd() {
        // the container says 1440 s but the PC's pipeline reached the real end at 1418.4 s
        w.onStatus("ready", ended = true, endSecs = 1418.4)
        observeAt(1_410_000, 1_440_000, buffering = true)
        assertEquals(Action.Wait, w.decide(now), "8 s before the PC's end: still playing what it made")
        observeAt(1_415_000, 1_440_000, buffering = false)
        assertEquals(Action.Wait, w.decide(now), "playing the last seconds")
        observeAt(1_415_500, 1_440_000, buffering = true)
        assertEquals(Action.Ended, w.decide(now))
    }

    @Test
    fun aSeekBackAfterTheEndPlaysAndCanEndAgain() {
        observeAt(1_437_000, 1_440_000, buffering = true)
        w.onTerminal()
        assertEquals(Action.Ended, w.decide(now))
        observeAt(1_200_000, 1_440_000, buffering = false) // the user seeks back
        repeat(10) { observeAt(pos + 500, 1_440_000, buffering = false) }
        assertEquals(Action.Wait, w.decide(now))
        observeAt(1_437_500, 1_440_000, buffering = true)
        w.onTerminal()
        assertEquals(Action.Ended, w.decide(now))
    }

    @Test
    fun aShortSeekDoesNotRenewOnTheBufferingThatFollows() {
        // ±10 s inside the session's playlist is a plain Media3 seek; the PC may need a few seconds for it
        run(5.0, playing = true)
        pos += 10_000
        run(8.0, playing = false)
        assertEquals(Action.Wait, w.decide(now))
        run(5.0, playing = true)
        assertEquals(Action.Wait, w.decide(now))
    }

    @Test
    fun aNewLoadForgetsThePendingTriggerButKeepsTheAttempts() {
        w.onTerminal()
        w.onRenewing(now)
        w.onTerminal()
        w.onLoad()
        now += 60_000
        assertEquals(Action.Wait, w.decide(now))
        assertEquals(1, w.attempts)
    }
}

/** 0.4.1: the Link buffering card shows after 800 ms of buffering and hides after 1 s without it. */
class BufferingDebounceTest {
    @Test
    fun shortFlipsNeverShowTheCard() {
        val d = BufferingDebounce()
        d.reset(0)
        var t = 0L
        // the incident: READY / BUFFERING alternating every ~0.5 s
        repeat(20) { i ->
            t += 500
            assertFalse(d.update(i % 2 == 0, t), "flip $i")
        }
    }

    @Test
    fun aRealStallShowsItAndPlaybackHidesItAfterASecond() {
        val d = BufferingDebounce()
        d.reset(0)
        assertFalse(d.update(true, 100))
        assertEquals(900L, d.nextChangeAt())
        assertFalse(d.update(true, 899))
        assertTrue(d.update(true, 900))
        assertNull(d.nextChangeAt())
        // a READY blip between two retries keeps it up
        assertTrue(d.update(false, 2_000))
        assertTrue(d.update(true, 2_400))
        assertTrue(d.update(false, 3_000))
        assertEquals(4_000L, d.nextChangeAt())
        assertTrue(d.update(false, 3_999))
        assertFalse(d.update(false, 4_000))
    }

    @Test
    fun aNewLoadStartsHidden() {
        val d = BufferingDebounce()
        d.reset(0)
        d.update(true, 0)
        assertTrue(d.update(true, 1_000))
        d.reset(5_000)
        assertFalse(d.update(false, 5_000))
        assertNull(d.nextChangeAt())
    }
}
