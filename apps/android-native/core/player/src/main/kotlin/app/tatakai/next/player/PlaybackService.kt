// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.session.DefaultMediaNotificationProvider
import androidx.media3.session.MediaSession
import androidx.media3.session.MediaSessionService
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

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
 * headset buttons control it, for Media3 and libmpv alike. Only while `AppSettings.backgroundPlayback` is on: the player screen calls
 * [start] when playback begins. The service stops itself (and releases its session) when [ActivePlayback.player] becomes null, when the
 * playback ends, and when the task is removed ([onTaskRemoved], like the Enigmata reference). Notification: Media3's
 * `DefaultMediaNotificationProvider` (play / pause / seek, title and artwork from the item metadata = [ActivePlayback.metadata]).
 *
 * Patterns adapted from Enigmata `app/src/main/java/app/enigmata/ui/player/MediaPlaybackService.kt` (same author, AGPL-3.0-or-later, see
 * THIRD_PARTY_NOTICES.md §5): one session per published player "generation" (re-created when the engine changes), an idempotent
 * [shutdown] that pauses the engine, and a gate so no notification / session update lands after shutdown.
 */
@UnstableApi
class PlaybackService : MediaSessionService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var session: MediaSession? = null
    private var sessionPlayer: Player? = null
    private var generation = 0

    /** Enigmata R12c: nothing may update the notification / session after [shutdown]. */
    @Volatile
    private var shutDown = false

    private val endListener = object : Player.Listener {
        override fun onPlaybackStateChanged(playbackState: Int) {
            // Playback ended (and no loop): the screen shows its next-episode card; the background session is over.
            if (playbackState == Player.STATE_ENDED) releaseSession(stopService = true)
        }
    }

    override fun onCreate() {
        super.onCreate()
        setMediaNotificationProvider(
            DefaultMediaNotificationProvider.Builder(this).setChannelName(R.string.player_service_channel).build(),
        )
        scope.launch { ActivePlayback.player.collect { bind(it) } }
        scope.launch {
            ActivePlayback.metadata.collect { if (!shutDown && session != null) triggerNotificationUpdate() }
        }
    }

    override fun onGetSession(controllerInfo: MediaSession.ControllerInfo): MediaSession? = if (shutDown) null else session

    override fun onUpdateNotification(session: MediaSession, startInForegroundRequired: Boolean) {
        if (shutDown) return
        super.onUpdateNotification(session, startInForegroundRequired)
    }

    /** One session per published player; a different player (other engine / episode) gets a fresh session. */
    private fun bind(player: Player?) {
        if (shutDown || player === sessionPlayer) return
        releaseSession(stopService = false)
        if (player == null) {
            stopSelf()
            return
        }
        generation++
        val s = MediaSession.Builder(this, player)
            .setId("tatakai-playback-$generation")
            .apply { sessionActivity()?.let { setSessionActivity(it) } }
            .build()
        player.addListener(endListener)
        session = s
        sessionPlayer = player
        addSession(s)
    }

    private fun releaseSession(stopService: Boolean) {
        val s = session
        sessionPlayer?.removeListener(endListener)
        session = null
        sessionPlayer = null
        if (s != null) {
            runCatching { removeSession(s) }
            s.release()
        }
        if (stopService) stopSelf()
    }

    /** Opens the app where it was (the player screen) from the notification. */
    private fun sessionActivity(): PendingIntent? {
        val launch = packageManager.getLaunchIntentForPackage(packageName) ?: return null
        launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_RESET_TASK_IF_NEEDED)
        return PendingIntent.getActivity(this, 0, launch, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    /** Idempotent: pause the engine, drop the session, stop. */
    private fun shutdown() {
        if (shutDown) return
        shutDown = true
        runCatching { sessionPlayer?.pause() }
        releaseSession(stopService = true)
    }

    /** Closing the app from Recents stops playback (acceptance (c) of 15 §5 p). */
    override fun onTaskRemoved(rootIntent: Intent?) {
        shutdown()
    }

    override fun onDestroy() {
        shutDown = true
        releaseSession(stopService = false)
        scope.cancel()
        super.onDestroy()
    }

    companion object {
        /** Started by the player screen when playback begins with `AppSettings.backgroundPlayback` on. */
        fun start(context: Context) {
            if (ActivePlayback.player.value == null) return
            runCatching { context.startService(Intent(context, PlaybackService::class.java)) }
        }
    }
}
