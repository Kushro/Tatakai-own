// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import app.tatakai.next.link.LinkApiError
import app.tatakai.next.link.LinkError
import app.tatakai.next.link.linkErrorOrNull
import app.tatakai.next.model.RemoteSession
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeout

/** BUG-131: why a Link profile change did not happen (each one has its own notice). */
enum class LinkProfileFailure {
    /** The PC did not answer within [LinkProfileChange.RENEW_TIMEOUT_MS]. */
    TIMEOUT,

    /** No connection with the PC (offline, PC asleep). */
    OFFLINE,

    /** The PC no longer has this session (or its source) and cannot recreate it: send it again from the PC. */
    SESSION_ENDED,

    /** The PC answered with an error (profile unavailable, source unreadable...). */
    PC_ERROR,
}

/** BUG-131: every way a tap on a profile of the in-player sheet ends; the player shows each one. */
sealed interface LinkProfileResult {
    /** The new session [sessionId] exists with the profile: reload it at [positionMs]. */
    data class Changed(val sessionId: String, val profileId: String, val positionMs: Long) : LinkProfileResult

    /** The tapped profile is the one in use: nothing to do. */
    data object SameProfile : LinkProfileResult

    /** A change is already running (repeated tap). */
    data object Busy : LinkProfileResult

    /** Nothing plays from the PC (no Link session in this player). */
    data object NoSession : LinkProfileResult

    data class Failed(val reason: LinkProfileFailure) : LinkProfileResult
}

/**
 * BUG-131 / BUG-022: changes the profile of the Link session played here. One change at a time ([changing] is true while it
 * runs, released in `finally` whatever happens); the PC's answer is bounded by [RENEW_TIMEOUT_MS]; after a new session is
 * created it waits (at most [READY_WAIT_MS]) until the PC says it is ready, so the player does not open a playlist that answers
 * `session_not_ready`. Every request ends in exactly one [LinkProfileResult] given to [onResult]; [log] gets one line per request
 * and one per result (logcat `TatakaiLink`).
 */
internal class LinkProfileChange(
    private val scope: CoroutineScope,
    private val renew: suspend (sessionId: String, startSecs: Double, profileId: String) -> Result<RemoteSession>,
    /** true = ready, false = the PC failed / closed it, null = still preparing or the PC could not be asked. */
    private val ready: suspend (sessionId: String) -> Boolean?,
    private val log: (String) -> Unit,
    private val renewTimeoutMs: Long = RENEW_TIMEOUT_MS,
    private val readyWaitMs: Long = READY_WAIT_MS,
) {
    private val _changing = MutableStateFlow(false)
    val changing: StateFlow<Boolean> = _changing.asStateFlow()

    /**
     * Asks the PC for [profileId] on [sessionId] (null = no Link session) at [positionMs]. [onStarted] runs when the request
     * really goes out (the screen shows "Cambiando perfil…"); [onResult] runs once with the outcome.
     */
    fun request(
        sessionId: String?,
        currentProfile: String?,
        profileId: String,
        positionMs: Long,
        onStarted: () -> Unit,
        onResult: (LinkProfileResult) -> Unit,
    ) {
        log("profile change requested session=${sessionId ?: "-"} from=${currentProfile ?: "-"} to=$profileId at=${positionMs}ms")
        val early = when {
            sessionId == null -> LinkProfileResult.NoSession
            profileId == currentProfile -> LinkProfileResult.SameProfile
            _changing.value -> LinkProfileResult.Busy
            else -> null
        }
        if (early != null || sessionId == null) {
            finish(sessionId, profileId, early ?: LinkProfileResult.NoSession, onResult)
            return
        }
        _changing.value = true
        onStarted()
        scope.launch {
            var result: LinkProfileResult = LinkProfileResult.Failed(LinkProfileFailure.PC_ERROR)
            try {
                result = run(sessionId, profileId, positionMs)
            } catch (e: CancellationException) {
                // the player closed while the change ran: nothing to show
                log("profile change cancelled session=$sessionId to=$profileId")
                throw e
            } finally {
                _changing.value = false
            }
            finish(sessionId, profileId, result, onResult)
        }
    }

    private suspend fun run(sessionId: String, profileId: String, positionMs: Long): LinkProfileResult {
        val renewed = try {
            withTimeout(renewTimeoutMs) { renew(sessionId, positionMs / 1000.0, profileId) }
        } catch (_: TimeoutCancellationException) {
            return LinkProfileResult.Failed(LinkProfileFailure.TIMEOUT)
        }
        val session = renewed.getOrElse { e -> return LinkProfileResult.Failed(failureOf(e)) }
        // the PC answers the create at once and prepares the first segments afterwards (2-10 s): wait for it here
        var waited = 0L
        while (waited < readyWaitMs) {
            when (ready(session.sessionId)) {
                true -> break
                false -> return LinkProfileResult.Failed(LinkProfileFailure.PC_ERROR)
                null -> {
                    delay(READY_POLL_MS)
                    waited += READY_POLL_MS
                }
            }
        }
        return LinkProfileResult.Changed(session.sessionId, profileId, positionMs)
    }

    private fun finish(sessionId: String?, profileId: String, result: LinkProfileResult, onResult: (LinkProfileResult) -> Unit) {
        val text = when (result) {
            is LinkProfileResult.Changed -> "changed new=${result.sessionId}"
            is LinkProfileResult.Failed -> "failed reason=${result.reason}"
            LinkProfileResult.SameProfile -> "same profile"
            LinkProfileResult.Busy -> "busy"
            LinkProfileResult.NoSession -> "no session"
        }
        log("profile change result session=${sessionId ?: "-"} to=$profileId $text")
        onResult(result)
    }

    companion object {
        const val RENEW_TIMEOUT_MS = 20_000L
        const val READY_WAIT_MS = 20_000L
        const val READY_POLL_MS = 500L

        fun failureOf(e: Throwable): LinkProfileFailure = when (val err = e.linkErrorOrNull()) {
            is LinkError.Transport.Refused, is LinkError.Transport.Timeout, LinkError.NotPaired -> LinkProfileFailure.OFFLINE
            is LinkError.Api -> when (err.error) {
                LinkApiError.SessionGone, LinkApiError.SessionNotFound, LinkApiError.RequestInvalid -> LinkProfileFailure.SESSION_ENDED
                else -> LinkProfileFailure.PC_ERROR
            }
            else -> LinkProfileFailure.PC_ERROR
        }
    }
}
