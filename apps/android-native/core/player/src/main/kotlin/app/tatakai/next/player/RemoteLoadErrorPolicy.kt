// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.upstream.DefaultLoadErrorHandlingPolicy

/**
 * Load errors of a Link (PC) session: a wifi blip or the PC waking up must not end playback. Media3's default gives up on a
 * playlist or segment after 3 retries (~6 s) and the player stops on an error; this keeps retrying the same session for about a
 * minute (waits of 0, 1, 2, 3, 4 and then 5 s), well inside the 120 s the PC keeps a session without heartbeat
 * (`tk-link` `IDLE_TIMEOUT_SECS`). Errors the default never retries (parser, file not found, ...) still fail at once.
 */
@UnstableApi
class RemoteLoadErrorPolicy : DefaultLoadErrorHandlingPolicy() {
    override fun getMinimumLoadableRetryCount(dataType: Int): Int = RETRIES

    companion object {
        /** 0 + 1 + 2 + 3 + 4 + 11 × 5 s ≈ 65 s of retries. */
        const val RETRIES = 16
    }
}
