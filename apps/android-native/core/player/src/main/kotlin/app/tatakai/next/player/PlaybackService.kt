// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import androidx.media3.common.Player
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

// Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.2), 2026-11-09 amendment (plan 14 §0.1 #12):
// background playback with Media3 AND libmpv. Owner: worker p.

/** What the media notification / lock screen shows for the current episode (poster, title, "Episodio N"). */
data class PlaybackMetadata(val title: String, val subtitle: String?, val artworkUri: String?)

/**
 * The engine that is playing right now, published for [PlaybackService]. Both engines go through a Media3 [Player]: the Media3 engine
 * publishes its `ExoPlayer` ([PlaybackController]); the libmpv engine publishes `MpvPlayerAdapter` (:core:player-mpv), a
 * `SimpleBasePlayer` that observes the mpv core the way Enigmata's `MediaPlaybackService` observes `MPVLib` (pause, time-pos, duration,
 * media-title) and forwards play / pause / seek / stop to it. One player at a time; [publish] with null when the session is released.
 */
object ActivePlayback {
    private val _player = MutableStateFlow<Player?>(null)
    val player: StateFlow<Player?> = _player.asStateFlow()

    private val _metadata = MutableStateFlow<PlaybackMetadata?>(null)
    val metadata: StateFlow<PlaybackMetadata?> = _metadata.asStateFlow()

    /** Main thread. Publishing a different player while one is set replaces it (the service rebuilds its session). */
    fun publish(player: Player?) {
        _player.value = player
    }

    fun setMetadata(metadata: PlaybackMetadata?) {
        _metadata.value = metadata
    }
}

/**
 * Background playback service (foreground type `mediaPlayback`, declared in this module's manifest): publishes a `MediaSession` built on
 * [ActivePlayback.player] so audio keeps playing when the app leaves the foreground and the system media notification / lock screen /
 * headset buttons control it, for Media3 and libmpv alike. Only while `AppSettings.backgroundPlayback` is on; the player screen starts
 * it when playback begins and the service stops itself (and releases the session) when [ActivePlayback.player] becomes null, playback
 * ends, or the task is removed (`onTaskRemoved`, like the Enigmata reference). Notification: `DefaultMediaNotificationProvider` with
 * [ActivePlayback.metadata]; nothing is updated after the service shut down (Enigmata's notification gate). Until worker p wires it,
 * [onGetSession] rejects every controller (null) so an exported service never crashes.
 */
class PlaybackService : MediaSessionService() {
    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = null
}
