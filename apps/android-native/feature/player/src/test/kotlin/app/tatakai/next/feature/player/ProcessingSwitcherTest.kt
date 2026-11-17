// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.link.LinkApiError
import app.tatakai.next.link.LinkError
import app.tatakai.next.link.toAppException
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.ProcessingLocation
import app.tatakai.next.model.ProcessingLocation.MIXED
import app.tatakai.next.model.ProcessingLocation.PC
import app.tatakai.next.model.ProcessingLocation.PHONE
import app.tatakai.next.model.RemoteSession
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.advanceTimeBy
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** 0.4.0: "Procesar en" switches keep the position and the audio, and a failure leaves the video where it plays. */
@OptIn(ExperimentalCoroutinesApi::class)
class ProcessingSwitcherTest {
    private class FakePort : ProcessingSwitcher.Port {
        var position = 600_000L
        var audio: Int? = 2
        var source: LinkSource? = LinkSource.Url("https://cdn.example/ep4.m3u8", mapOf("Referer" to "https://site"))
        var start: suspend (LinkSource, String?, Double, Int?) -> Result<RemoteSession> = { _, _, _, _ -> Result.success(session("new")) }
        var renew: suspend (String, Double, String?) -> Result<RemoteSession> = { _, _, _ -> Result.success(session("renewed")) }
        var readyAnswers = ArrayDeque<Boolean?>()
        var remoteOk = true
        var localOk = true
        val starts = mutableListOf<Triple<LinkSource, String?, Pair<Double, Int?>>>()
        val renews = mutableListOf<Triple<String, Double, String?>>()
        val playedRemote = mutableListOf<Triple<ProcessingLocation, String, Long>>()
        val playedLocal = mutableListOf<Long>()
        val stopped = mutableListOf<String>()

        override fun positionMs() = position
        override fun audioTrackIndex() = audio
        override suspend fun currentLinkSource() = source
        override suspend fun startAt(source: LinkSource, profileId: String?, startSecs: Double, audioTrack: Int?): Result<RemoteSession> {
            starts += Triple(source, profileId, startSecs to audioTrack)
            return start(source, profileId, startSecs, audioTrack)
        }
        override suspend fun renew(sessionId: String, startSecs: Double, profileId: String?): Result<RemoteSession> {
            renews += Triple(sessionId, startSecs, profileId)
            return renew.invoke(sessionId, startSecs, profileId)
        }
        override suspend fun ready(sessionId: String): Boolean? = if (readyAnswers.isEmpty()) true else readyAnswers.removeFirst()
        override fun playRemote(location: ProcessingLocation, session: RemoteSession, profileId: String?): Boolean {
            playedRemote += Triple(location, session.sessionId, position)
            return remoteOk
        }
        override fun playLocal(positionMs: Long): Boolean {
            playedLocal += positionMs
            return localOk
        }
        override fun stop(sessionId: String) {
            stopped += sessionId
        }
    }

    private companion object {
        fun session(id: String) = RemoteSession(id, 1, "https://pc/v1/sessions/$id/master.m3u8", 1420.0, 4, 120)
    }

    private fun TestScope.switcher(port: FakePort) = ProcessingSwitcher(backgroundScope, port)

    private fun ProcessingSwitcher.go(from: ProcessingLocation, to: ProcessingLocation, session: String?, profile: String?, out: MutableList<ProcessingSwitchResult>) =
        request(from, to, session, profile) { out += it }

    @Test
    fun phoneToPcStartsAtThePositionWithTheAudioAndLoadsWhereThePhoneGot() = runTest {
        val port = FakePort()
        val out = mutableListOf<ProcessingSwitchResult>()
        val s = switcher(port)
        port.readyAnswers.addAll(listOf(null, null, true)) // the PC prepares for 1 s
        s.go(PHONE, PC, null, "p-anime4k-1080", out)
        runCurrent()
        assertEquals(PC, s.switching.value)
        // the phone keeps playing meanwhile
        port.position = 601_500L
        advanceTimeBy(1_100)
        runCurrent()
        assertEquals(listOf<ProcessingSwitchResult>(ProcessingSwitchResult.Switched(PC, "new", "p-anime4k-1080")), out)
        val (src, profile, at) = port.starts.single()
        assertEquals(port.source, src)
        assertEquals("p-anime4k-1080", profile)
        assertEquals(600.0 to 2, at, "the session starts where the phone was, with its audio stream")
        assertEquals(Triple(PC, "new", 601_500L), port.playedRemote.single(), "loaded at the point the phone reached")
        assertNull(s.switching.value)
        assertEquals(emptyList<String>(), port.stopped)
    }

    @Test
    fun aFailedStartLeavesThePhonePlayingWithTheReason() = runTest {
        val port = FakePort()
        val out = mutableListOf<ProcessingSwitchResult>()
        val s = switcher(port)
        port.start = { _, _, _, _ -> Result.failure(LinkError.Api(LinkApiError.SourceUnavailable).toAppException()) }
        s.go(PHONE, PC, null, null, out)
        runCurrent()
        assertEquals(listOf<ProcessingSwitchResult>(ProcessingSwitchResult.Failed(PC, ProcessingFailure.SOURCE_REFUSED)), out)
        assertEquals(0, port.playedRemote.size)
        assertEquals(0, port.playedLocal.size, "nothing to resume: the phone never stopped")

        port.start = { _, _, _, _ -> Result.failure(LinkError.Transport.Timeout.toAppException()) }
        s.go(PHONE, MIXED, null, "p-anime4k-1080", out)
        runCurrent()
        assertEquals(ProcessingSwitchResult.Failed(MIXED, ProcessingFailure.OFFLINE), out.last())

        // nothing sendable (the source went away)
        port.source = null
        s.go(PHONE, PC, null, null, out)
        runCurrent()
        assertEquals(ProcessingSwitchResult.Failed(PC, ProcessingFailure.SOURCE_REFUSED), out.last())
    }

    @Test
    fun aHungPcTimesOutAndASessionThatCannotPlayIsClosed() = runTest {
        val port = FakePort()
        val out = mutableListOf<ProcessingSwitchResult>()
        val s = ProcessingSwitcher(backgroundScope, port, startTimeoutMs = 5_000)
        port.start = { _, _, _, _ -> awaitCancellation() }
        s.go(PHONE, PC, null, null, out)
        advanceTimeBy(5_001)
        runCurrent()
        assertEquals(listOf<ProcessingSwitchResult>(ProcessingSwitchResult.Failed(PC, ProcessingFailure.TIMEOUT)), out)
        assertNull(s.switching.value, "the next tap works")

        // the PC made the session but its stream cannot be opened: it is closed, the phone goes on
        port.start = { _, _, _, _ -> Result.success(session("orphan")) }
        port.remoteOk = false
        s.go(PHONE, PC, null, null, out)
        runCurrent()
        assertEquals(ProcessingSwitchResult.Failed(PC, ProcessingFailure.PC_ERROR), out.last())
        assertEquals(listOf("orphan"), port.stopped)

        // the PC says the new session failed while preparing
        port.remoteOk = true
        port.readyAnswers.add(false)
        s.go(PHONE, PC, null, null, out)
        runCurrent()
        assertEquals(ProcessingSwitchResult.Failed(PC, ProcessingFailure.PC_ERROR), out.last())
    }

    @Test
    fun pcToMixedRenewsTheSessionWithTheUpscaleProfile() = runTest {
        val port = FakePort()
        val out = mutableListOf<ProcessingSwitchResult>()
        val s = switcher(port)
        s.go(PC, MIXED, "old", "p-anime4k-1080", out)
        runCurrent()
        assertEquals(listOf<Triple<String, Double, String?>>(Triple("old", 600.0, "p-anime4k-1080")), port.renews)
        assertEquals(Triple(MIXED, "renewed", 600_000L), port.playedRemote.single())
        assertEquals(ProcessingSwitchResult.Switched(MIXED, "renewed", "p-anime4k-1080"), out.single())
        assertEquals(0, port.starts.size, "the PC already has the source")

        // and back to PC with the profile used before
        s.go(MIXED, PC, "renewed", "p-anime4k-lsfg-1080-x2", out)
        runCurrent()
        assertEquals(Triple("renewed", 600.0, "p-anime4k-lsfg-1080-x2"), port.renews.last())
        assertEquals(PC, port.playedRemote.last().first)
    }

    @Test
    fun pcToPhoneResumesAtTheSessionPositionAndClosesTheSession() = runTest {
        val port = FakePort()
        val out = mutableListOf<ProcessingSwitchResult>()
        val s = switcher(port)
        port.position = 812_000L
        s.go(PC, PHONE, "sess", null, out)
        assertEquals(listOf(812_000L), port.playedLocal)
        assertEquals(listOf("sess"), port.stopped)
        assertEquals(ProcessingSwitchResult.Switched(PHONE, null, null), out.single())

        // nothing to play on the phone: the PC session stays
        port.localOk = false
        s.go(MIXED, PHONE, "sess2", null, out)
        assertEquals(ProcessingSwitchResult.Failed(PHONE, ProcessingFailure.LOCAL_UNAVAILABLE), out.last())
        assertEquals(listOf("sess"), port.stopped)
    }

    @Test
    fun oneSwitchAtATimeAndCancellingClosesWhatThePcMade() = runTest {
        val port = FakePort()
        val out = mutableListOf<ProcessingSwitchResult>()
        val s = switcher(port)
        port.readyAnswers.addAll(List(10) { null })
        s.go(PHONE, PC, null, null, out)
        runCurrent()
        s.go(PHONE, MIXED, null, null, out)
        assertEquals(listOf<ProcessingSwitchResult>(ProcessingSwitchResult.Busy), out)
        s.go(PC, PC, "x", null, out)
        assertEquals(ProcessingSwitchResult.Busy, out.last())
        s.cancel()
        runCurrent()
        assertEquals(ProcessingSwitchResult.Cancelled, out.last())
        assertEquals(listOf("new"), port.stopped, "the session the PC was preparing is closed")
        assertNull(s.switching.value)
        assertEquals(0, port.playedRemote.size)
        // same location / no session
        s.go(PC, PC, "x", null, out)
        assertEquals(ProcessingSwitchResult.Same, out.last())
        s.go(PC, MIXED, null, null, out)
        assertEquals(ProcessingSwitchResult.Failed(MIXED, ProcessingFailure.PC_ERROR), out.last())
    }
}
