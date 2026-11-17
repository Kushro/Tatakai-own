// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.link.LinkApiError
import app.tatakai.next.link.LinkError
import app.tatakai.next.link.linkErrorOrNull
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.ProcessingLocation
import app.tatakai.next.model.RemoteSession
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout

/** How a "Procesar en" switch ended. */
sealed interface ProcessingSwitchResult {
    /** Now processed at [to]; [sessionId] / [profileId] = the Link session (null for PHONE). */
    data class Switched(val to: ProcessingLocation, val sessionId: String?, val profileId: String?) : ProcessingSwitchResult

    data object Same : ProcessingSwitchResult
    data object Busy : ProcessingSwitchResult

    /** The switch was cancelled (another choice, the player closed). Nothing changed. */
    data object Cancelled : ProcessingSwitchResult

    data class Failed(val to: ProcessingLocation, val reason: ProcessingFailure) : ProcessingSwitchResult
}

/**
 * 0.4.0: switches the player between Phone, PC and Mixto keeping the position (and the audio stream when going to the PC).
 * The video keeps playing where it is until the new side is ready, so a failure changes nothing:
 *
 * - Phone -> PC / Mixto: a new Link session of the current source ([Port.currentLinkSource]) at the current position with the
 *   source's audio stream, the PC's answer bounded by [startTimeoutMs] (a torrent waits for its metadata on the PC); once the
 *   PC says the session is ready (at most [readyWaitMs]) the player loads it at the position it reached meanwhile
 *   ([Port.playRemote]: the PC serves a few seconds past its start without restarting).
 * - PC <-> Mixto: the same session's source with the other profile ([Port.renew], tk-link `session` source).
 * - PC / Mixto -> Phone: the original source plays again at the session's position ([Port.playLocal]) and the session is
 *   closed (best effort: the PC closes idle sessions after 120 s anyway).
 *
 * One switch at a time ([switching]); every request ends in exactly one [ProcessingSwitchResult].
 */
internal class ProcessingSwitcher(
    private val scope: CoroutineScope,
    private val port: Port,
    private val log: (String) -> Unit = {},
    private val startTimeoutMs: Long = START_TIMEOUT_MS,
    private val renewTimeoutMs: Long = LinkProfileChange.RENEW_TIMEOUT_MS,
    private val readyWaitMs: Long = LinkProfileChange.READY_WAIT_MS,
) {
    /** What the switcher needs from the player (fakes in tests). */
    interface Port {
        fun positionMs(): Long

        /** Index of the selected audio stream of the current phone playback (null = the first / unknown). */
        fun audioTrackIndex(): Int?

        /** The current phone playback as a Link source; null = nothing sendable. */
        suspend fun currentLinkSource(): LinkSource?

        suspend fun startAt(source: LinkSource, profileId: String?, startSecs: Double, audioTrack: Int?): Result<RemoteSession>

        suspend fun renew(sessionId: String, startSecs: Double, profileId: String?): Result<RemoteSession>

        /** true = ready, false = failed / closed, null = still preparing or the PC cannot be asked. */
        suspend fun ready(sessionId: String): Boolean?

        /** Loads [session] for [location] at the current position; false when its stream cannot be opened. */
        fun playRemote(location: ProcessingLocation, session: RemoteSession, profileId: String?): Boolean

        /** Plays the original source on the phone at [positionMs]; false when there is nothing to play. */
        fun playLocal(positionMs: Long): Boolean

        /** Closes a session that is no longer played (best effort). */
        fun stop(sessionId: String)
    }

    private val _switching = MutableStateFlow<ProcessingLocation?>(null)
    val switching: StateFlow<ProcessingLocation?> = _switching.asStateFlow()
    private var job: Job? = null

    /**
     * Switches from [from] to [to]. [sessionId] = the Link session played now (PC / Mixto), [profileId] = the profile to use at
     * [to] (null = the preferred / default one). [onResult] runs once.
     */
    fun request(
        from: ProcessingLocation,
        to: ProcessingLocation,
        sessionId: String?,
        profileId: String?,
        onResult: (ProcessingSwitchResult) -> Unit,
    ) {
        val early = when {
            _switching.value != null -> ProcessingSwitchResult.Busy
            from == to -> ProcessingSwitchResult.Same
            from != ProcessingLocation.PHONE && sessionId == null -> ProcessingSwitchResult.Failed(to, ProcessingFailure.PC_ERROR)
            else -> null
        }
        log("processing switch requested from=$from to=$to session=${sessionId ?: "-"} profile=${profileId ?: "-"}")
        if (early != null) {
            finish(to, early, onResult)
            return
        }
        if (to == ProcessingLocation.PHONE) {
            // nothing to wait for: the phone plays at once and the session is closed behind it
            val position = port.positionMs()
            val played = port.playLocal(position)
            if (played && sessionId != null) port.stop(sessionId)
            finish(to, if (played) ProcessingSwitchResult.Switched(to, null, null) else ProcessingSwitchResult.Failed(to, ProcessingFailure.LOCAL_UNAVAILABLE), onResult)
            return
        }
        _switching.value = to
        job = scope.launch {
            var created: RemoteSession? = null
            var result: ProcessingSwitchResult = ProcessingSwitchResult.Failed(to, ProcessingFailure.PC_ERROR)
            try {
                val positionSecs = port.positionMs() / 1000.0
                val started = if (from == ProcessingLocation.PHONE) {
                    val source = port.currentLinkSource()
                    if (source == null) {
                        Result.failure(IllegalStateException("nothing sendable"))
                    } else {
                        bounded(startTimeoutMs) { port.startAt(source, profileId, positionSecs, port.audioTrackIndex()) }
                    }
                } else {
                    bounded(renewTimeoutMs) { port.renew(sessionId!!, positionSecs, profileId) }
                }
                result = started.fold(
                    onSuccess = { session ->
                        created = session
                        if (!waitReady(session.sessionId)) {
                            ProcessingSwitchResult.Failed(to, ProcessingFailure.PC_ERROR)
                        } else if (!port.playRemote(to, session, profileId)) {
                            ProcessingSwitchResult.Failed(to, ProcessingFailure.PC_ERROR)
                        } else {
                            created = null // played: owned by the player now
                            ProcessingSwitchResult.Switched(to, session.sessionId, profileId)
                        }
                    },
                    onFailure = { e -> ProcessingSwitchResult.Failed(to, failureOf(e)) },
                )
            } catch (e: CancellationException) {
                created?.let { port.stop(it.sessionId) }
                _switching.value = null
                finish(to, ProcessingSwitchResult.Cancelled, onResult)
                throw e
            } finally {
                _switching.value = null
            }
            // Phone -> PC failed after the PC made a session the player does not play: close it (PC <-> Mixto keeps the
            // played session, which the PC replaced only if the new one exists)
            if (from == ProcessingLocation.PHONE) created?.let { port.stop(it.sessionId) }
            finish(to, result, onResult)
        }
    }

    /** Cancels a running switch (the user chose another location, the player closes). */
    fun cancel() {
        job?.cancel()
        job = null
    }

    private suspend fun bounded(timeoutMs: Long, block: suspend () -> Result<RemoteSession>): Result<RemoteSession> = try {
        withTimeout(timeoutMs) { block() }
    } catch (e: TimeoutCancellationException) {
        Result.failure(SwitchTimeout())
    }

    private suspend fun waitReady(sessionId: String): Boolean {
        var waited = 0L
        while (waited < readyWaitMs) {
            when (port.ready(sessionId)) {
                true -> return true
                false -> return false
                null -> {
                    delay(LinkProfileChange.READY_POLL_MS)
                    waited += LinkProfileChange.READY_POLL_MS
                }
            }
        }
        // still preparing: load it anyway, the player's own start watch (BUG-131) retries once and then shows the error
        return true
    }

    private fun finish(to: ProcessingLocation, result: ProcessingSwitchResult, onResult: (ProcessingSwitchResult) -> Unit) {
        val text = when (result) {
            is ProcessingSwitchResult.Switched -> "switched session=${result.sessionId ?: "-"}"
            is ProcessingSwitchResult.Failed -> "failed reason=${result.reason}"
            ProcessingSwitchResult.Same -> "same"
            ProcessingSwitchResult.Busy -> "busy"
            ProcessingSwitchResult.Cancelled -> "cancelled"
        }
        log("processing switch result to=$to $text")
        onResult(result)
    }

    private class SwitchTimeout : Exception("timeout")

    companion object {
        /** A torrent source makes the PC wait up to 75 s for its metadata (LinkClient.TORRENT_CREATE_SECS = 90). */
        const val START_TIMEOUT_MS = 95_000L

        fun failureOf(e: Throwable): ProcessingFailure {
            if (e is SwitchTimeout) return ProcessingFailure.TIMEOUT
            if (e is IllegalStateException) return ProcessingFailure.SOURCE_REFUSED
            return when (val err = e.linkErrorOrNull()) {
                is LinkError.Transport.Refused, is LinkError.Transport.Timeout, LinkError.NotPaired -> ProcessingFailure.OFFLINE
                is LinkError.Api -> when (err.error) {
                    LinkApiError.SourceUnavailable, LinkApiError.UrlSourceDenied, LinkApiError.RequestInvalid -> ProcessingFailure.SOURCE_REFUSED
                    else -> ProcessingFailure.PC_ERROR
                }
                else -> ProcessingFailure.PC_ERROR
            }
        }
    }
}
