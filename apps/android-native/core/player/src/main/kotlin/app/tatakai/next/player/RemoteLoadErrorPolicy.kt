// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import androidx.media3.common.C
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.HttpDataSource
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy
import androidx.media3.exoplayer.upstream.LoadErrorHandlingPolicy
import java.io.IOException

/**
 * Load errors of a Link (PC) session: a wifi blip or the PC waking up must not end playback. Media3's default gives up on a
 * playlist or segment after 3 retries (~6 s) and the player stops on an error; this keeps retrying the same session for about a
 * minute (waits of 0, 1, 2, 3, 4 and then 5 s), well inside the 120 s the PC keeps a session without heartbeat
 * (`tk-link` `IDLE_TIMEOUT_SECS`). Errors the default never retries (parser, file not found, ...) still fail at once.
 *
 * BUG-138: a session the PC no longer has answers `404 session_not_found` / `410 session_gone`, which no retry fixes (r10: a
 * hand-off card played 2 min after the send sat 65 s on "preparing" retrying a 410). Before the load showed its first frame
 * ([failEndedAtOnce] true) that fails at once, so the player's recovery opens a new session right away. Once playing it keeps
 * the retries: during a profile change the PC closes the old session before the new one is ready, and the buffered video must
 * keep playing until the change switches to it. Timeouts, 5xx and "no connection" are always retried.
 */
@UnstableApi
class RemoteLoadErrorPolicy(
    private val failEndedAtOnce: () -> Boolean = { true },
    /** 0.4.0: whether a load is the PC's (a side-loaded web subtitle answering 404 says nothing about the session). */
    private val fromSession: (android.net.Uri?) -> Boolean = { true },
) : DefaultLoadErrorHandlingPolicy() {
    override fun getMinimumLoadableRetryCount(dataType: Int): Int = RETRIES

    override fun getRetryDelayMsFor(loadErrorInfo: LoadErrorHandlingPolicy.LoadErrorInfo): Long =
        if (endsSession(loadErrorInfo.exception) && failEndedAtOnce() && fromSession(loadErrorInfo.loadEventInfo.uri)) {
            C.TIME_UNSET
        } else {
            super.getRetryDelayMsFor(loadErrorInfo)
        }

    companion object {
        /** 0 + 1 + 2 + 3 + 4 + 11 × 5 s ≈ 65 s of retries. */
        const val RETRIES = 16

        /** `tk-link` statuses of a session it does not have (closed, reaped after 120 s idle, or the PC restarted). */
        private val ENDED_CODES = setOf(404, 410)

        /** The PC answered that this session does not exist (any other failure may be transient). */
        fun endsSession(e: IOException): Boolean = e is HttpDataSource.InvalidResponseCodeException && endsSession(e.responseCode)

        fun endsSession(responseCode: Int): Boolean = responseCode in ENDED_CODES
    }
}
