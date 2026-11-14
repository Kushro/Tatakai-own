// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.link.LinkApiError
import app.tatakai.next.link.LinkError
import app.tatakai.next.link.toAppException
import app.tatakai.next.model.RemoteSession
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** BUG-131: every tap on a profile of the in-player sheet ends in a visible result, and a stuck PC never blocks the next tap. */
@OptIn(ExperimentalCoroutinesApi::class)
class LinkProfileChangeTest {
    private class Recorder {
        val results = mutableListOf<LinkProfileResult>()
        val logs = mutableListOf<String>()
        var started = 0
        val renewCalls = mutableListOf<Triple<String, Double, String>>()
    }

    private fun session(id: String) = RemoteSession(id, 1, "https://pc/v1/sessions/$id/master.m3u8", 1420.0, 4, 120)

    private fun TestScope.changer(
        rec: Recorder,
        ready: suspend (String) -> Boolean? = { true },
        renew: suspend (String, Double, String) -> Result<RemoteSession>,
    ) = LinkProfileChange(
        scope = backgroundScope,
        renew = { id, secs, profile -> rec.renewCalls += Triple(id, secs, profile); renew(id, secs, profile) },
        ready = ready,
        log = { rec.logs += it },
    )

    private fun LinkProfileChange.tap(rec: Recorder, session: String? = "old", current: String? = "p-a4k", to: String = "p-none", at: Long = 949_000) =
        request(session, current, to, at, onStarted = { rec.started++ }, onResult = { rec.results += it })

    @Test
    fun aHungRenewTimesOutWithANoticeAndTheNextTapWorks() = runTest {
        val rec = Recorder()
        var hang = true
        val c = changer(rec) { _, _, _ -> if (hang) awaitCancellation() else Result.success(session("new")) }
        c.tap(rec)
        runCurrent()
        assertTrue(c.changing.value)
        advanceTimeBy(LinkProfileChange.RENEW_TIMEOUT_MS + 1)
        runCurrent()
        assertEquals(listOf<LinkProfileResult>(LinkProfileResult.Failed(LinkProfileFailure.TIMEOUT)), rec.results)
        assertFalse(c.changing.value, "the flag is released after a timeout")
        // retry
        hang = false
        c.tap(rec)
        runCurrent()
        assertEquals(LinkProfileResult.Changed("new", "p-none", 949_000), rec.results.last())
        assertEquals(2, rec.started)
        assertTrue(rec.logs.any { "failed reason=TIMEOUT" in it } && rec.logs.any { "changed new=new" in it })
    }

    @Test
    fun aRepeatedTapWhileChangingSaysBusyAndDoesNotAskThePcAgain() = runTest {
        val rec = Recorder()
        val answer = CompletableDeferred<Result<RemoteSession>>()
        val c = changer(rec) { _, _, _ -> answer.await() }
        c.tap(rec)
        runCurrent()
        c.tap(rec, to = "p-a4k-1080")
        runCurrent()
        assertEquals(listOf<LinkProfileResult>(LinkProfileResult.Busy), rec.results)
        assertEquals(1, rec.renewCalls.size)
        answer.complete(Result.success(session("new")))
        runCurrent()
        assertEquals(LinkProfileResult.Changed("new", "p-none", 949_000), rec.results.last())
        assertFalse(c.changing.value)
    }

    @Test
    fun successReloadsAtThePositionOfTheTapOnceThePcIsReady() = runTest {
        val rec = Recorder()
        var polls = 0
        // the PC answers the create at once and says "preparing" for 2 s (r6: master.m3u8 answered session_not_ready for 10 s)
        val c = changer(rec, ready = { polls++; if (polls < 5) null else true }) { _, _, _ -> Result.success(session("new")) }
        c.tap(rec, at = 949_500)
        runCurrent()
        assertTrue(rec.results.isEmpty(), "no result while the PC prepares")
        advanceTimeBy(LinkProfileChange.READY_POLL_MS * 5)
        runCurrent()
        assertEquals(listOf<LinkProfileResult>(LinkProfileResult.Changed("new", "p-none", 949_500)), rec.results)
        assertEquals(Triple("old", 949.5, "p-none"), rec.renewCalls.single())
    }

    @Test
    fun aSessionThatNeverGetsReadyStillLoadsAfterTheWait() = runTest {
        val rec = Recorder()
        val c = changer(rec, ready = { null }) { _, _, _ -> Result.success(session("new")) }
        c.tap(rec)
        advanceTimeBy(LinkProfileChange.READY_WAIT_MS + LinkProfileChange.READY_POLL_MS)
        runCurrent()
        // the player's own retries and the start watchdog take over from here
        assertEquals(LinkProfileResult.Changed("new", "p-none", 949_000), rec.results.single())
    }

    @Test
    fun earlyExitsAreVisibleToo() = runTest {
        val rec = Recorder()
        val c = changer(rec) { _, _, _ -> Result.success(session("new")) }
        c.tap(rec, session = null)
        c.tap(rec, to = "p-a4k")
        runCurrent()
        assertEquals(listOf(LinkProfileResult.NoSession, LinkProfileResult.SameProfile), rec.results)
        assertEquals(0, rec.started)
        assertTrue(rec.renewCalls.isEmpty())
    }

    @Test
    fun pcRefusalsAndAFailedSessionSayWhy() = runTest {
        val rec = Recorder()
        var error: LinkError = LinkError.Api(LinkApiError.SessionNotFound)
        val c = changer(rec, ready = { false }) { _, _, _ -> if (error is LinkError.NotPaired) Result.success(session("new")) else Result.failure(error.toAppException()) }
        c.tap(rec)
        runCurrent()
        error = LinkError.Transport.Refused
        c.tap(rec)
        runCurrent()
        error = LinkError.NotPaired // renew works, but the PC reports the new session failed
        c.tap(rec)
        runCurrent()
        assertEquals(
            listOf<LinkProfileResult>(
                LinkProfileResult.Failed(LinkProfileFailure.SESSION_ENDED),
                LinkProfileResult.Failed(LinkProfileFailure.OFFLINE),
                LinkProfileResult.Failed(LinkProfileFailure.PC_ERROR),
            ),
            rec.results,
        )
        assertFalse(c.changing.value)
    }
}
