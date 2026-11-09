// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService

/**
 * Parity wave A contract stub (docs/redesign/15-parity-wave-a-contract-android.md §3.2). Owner: worker p.
 *
 * Background playback: the Media3 session of the current [PlaybackController] is published through this `MediaSessionService`
 * (foreground type `mediaPlayback`, declared in this module's manifest), so audio keeps playing when the app leaves the foreground
 * and the system media notification / lock screen / headset buttons control it. Only when `AppSettings.backgroundPlayback` is on;
 * the service stops itself when playback ends or the player screen is closed. Until worker p wires it, [onGetSession] rejects every
 * controller (null) so an exported service never crashes.
 */
class PlaybackService : MediaSessionService() {
    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = null
}
