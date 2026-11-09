// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.net.Uri
import android.os.Handler
import android.os.Looper
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.PlaybackParameters
import androidx.media3.common.Player
import androidx.media3.common.SimpleBasePlayer
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.player.ActivePlayback
import app.tatakai.next.player.PlaybackMetadata
import com.google.common.util.concurrent.Futures
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.android.asCoroutineDispatcher
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.2, 2026-11-09 amendment). Owner: worker p.
 *
 * libmpv seen as a Media3 `Player`, so `PlaybackService` (:core:player) publishes one `MediaSession` for both engines (notification,
 * lock screen, headset buttons). [getState] is derived from [MpvEngine.state] ([MpvSessionFacts.of]: playWhenReady = !paused, playback
 * state from buffering / ended / error, position and duration from `time-pos` / `duration`) plus one `MediaItemData` with
 * [ActivePlayback.metadata]; play / pause / seek / stop / speed go to the engine. Every engine update calls [invalidateState] on the
 * adapter's looper. Patterns adapted from Enigmata `app/src/main/java/app/enigmata/ui/player/MediaPlaybackService.kt` (same author,
 * AGPL-3.0-or-later, THIRD_PARTY_NOTICES.md §5): observe the core instead of polling it, one observer per core "generation" (one adapter
 * per [MpvEngine]; a new engine publishes a new adapter), and no update after shutdown ([detach]).
 * `MpvPlayerSession` publishes this adapter through `ActivePlayback.publish` on load and null on release.
 */
@UnstableApi
class MpvPlayerAdapter(private val engine: MpvEngine, looper: Looper) : SimpleBasePlayer(looper) {
    private val scope = CoroutineScope(SupervisorJob() + Handler(looper).asCoroutineDispatcher())
    private var snapshot: MpvState = engine.state.value
    private var metadata: PlaybackMetadata? = ActivePlayback.metadata.value

    /** Enigmata's shutdown gate: after [detach] the adapter reports an idle, empty player and never touches the engine again. */
    private var detached = false

    init {
        scope.launch {
            engine.state.collect {
                snapshot = it
                if (!detached) invalidateState()
            }
        }
        scope.launch {
            ActivePlayback.metadata.collect {
                metadata = it
                if (!detached) invalidateState()
            }
        }
    }

    override fun getState(): State {
        val commands = Player.Commands.Builder().addAll(
            Player.COMMAND_PLAY_PAUSE,
            Player.COMMAND_STOP,
            Player.COMMAND_SEEK_IN_CURRENT_MEDIA_ITEM,
            Player.COMMAND_SEEK_BACK,
            Player.COMMAND_SEEK_FORWARD,
            Player.COMMAND_SEEK_TO_DEFAULT_POSITION,
            Player.COMMAND_SET_SPEED_AND_PITCH,
            Player.COMMAND_GET_CURRENT_MEDIA_ITEM,
            Player.COMMAND_GET_TIMELINE,
            Player.COMMAND_GET_METADATA,
            Player.COMMAND_RELEASE,
        ).build()
        if (detached) {
            return State.Builder().setAvailableCommands(Player.Commands.EMPTY).setPlaybackState(Player.STATE_IDLE).build()
        }
        val facts = MpvSessionFacts.of(snapshot)
        val md = MediaMetadata.Builder()
            .setTitle(metadata?.title)
            .setDisplayTitle(metadata?.title)
            .setArtist(metadata?.subtitle)
            .apply { metadata?.artworkUri?.let { setArtworkUri(Uri.parse(it)) } }
            .build()
        val uid = "mpv-${snapshot.loadId}"
        val item = MediaItemData.Builder(uid)
            .setMediaItem(MediaItem.Builder().setMediaId(uid).setMediaMetadata(md).build())
            .setMediaMetadata(md)
            .setDurationUs(if (facts.durationMs > 0) facts.durationMs * 1000 else C.TIME_UNSET)
            .setIsSeekable(facts.durationMs > 0)
            .build()
        val speed = snapshot.speed.toFloat().takeIf { it > 0f } ?: 1f
        return State.Builder()
            .setAvailableCommands(commands)
            .setPlaylist(listOf(item))
            .setCurrentMediaItemIndex(0)
            .setPlayWhenReady(facts.playWhenReady, Player.PLAY_WHEN_READY_CHANGE_REASON_USER_REQUEST)
            .setPlaybackState(facts.playbackState)
            .setIsLoading(facts.playbackState == Player.STATE_BUFFERING)
            .setPlaybackParameters(PlaybackParameters(speed))
            .setContentPositionMs(
                if (facts.playing) PositionSupplier.getExtrapolating(facts.positionMs, speed) else PositionSupplier.getConstant(facts.positionMs),
            )
            .setSeekBackIncrementMs(SEEK_INCREMENT_MS)
            .setSeekForwardIncrementMs(SEEK_INCREMENT_MS)
            .apply {
                if (facts.error != null) setPlayerError(PlaybackException(facts.error, null, PlaybackException.ERROR_CODE_UNSPECIFIED))
            }
            .build()
    }

    override fun handleSetPlayWhenReady(playWhenReady: Boolean): ListenableFuture<*> {
        if (!detached) if (playWhenReady) engine.play() else engine.pause()
        return Futures.immediateVoidFuture()
    }

    override fun handleSeek(mediaItemIndex: Int, positionMs: Long, seekCommand: Int): ListenableFuture<*> {
        if (!detached && positionMs != C.TIME_UNSET) engine.seekTo(positionMs.coerceAtLeast(0) / 1000.0)
        return Futures.immediateVoidFuture()
    }

    override fun handleSetPlaybackParameters(playbackParameters: PlaybackParameters): ListenableFuture<*> {
        if (!detached) engine.setSpeed(playbackParameters.speed.toDouble())
        return Futures.immediateVoidFuture()
    }

    /** "Stop" from the notification: pause (the screen owns the engine and decides when it is released). */
    override fun handleStop(): ListenableFuture<*> {
        if (!detached) engine.pause()
        return Futures.immediateVoidFuture()
    }

    /** A controller released the player: stop observing; the engine itself is released by `MpvPlayerSession`. */
    override fun handleRelease(): ListenableFuture<*> {
        detach()
        return Futures.immediateVoidFuture()
    }

    /** Called by `MpvPlayerSession.release` before the engine is destroyed: no more engine calls nor state updates. */
    fun detach() {
        if (detached) return
        detached = true
        scope.cancel()
    }

    private companion object {
        const val SEEK_INCREMENT_MS = 10_000L
    }
}

/**
 * What the media session needs from one [MpvState] (pure, unit-tested): Media3 playback state constants, play-when-ready, position and
 * duration in ms, the error text.
 */
data class MpvSessionFacts(
    val playWhenReady: Boolean,
    val playing: Boolean,
    val playbackState: Int,
    val positionMs: Long,
    val durationMs: Long,
    val error: String?,
) {
    companion object {
        fun of(s: MpvState): MpvSessionFacts {
            val state = when {
                s.error != null -> Player.STATE_IDLE
                s.ended && !s.loop -> Player.STATE_ENDED
                s.loadId == 0 -> Player.STATE_IDLE
                s.buffering || !s.started -> Player.STATE_BUFFERING
                else -> Player.STATE_READY
            }
            // mpv keeps `pause=no` while it buffers: what the user asked for is "playing" unless paused or finished.
            val wantsPlay = s.playing || (s.buffering && s.error == null && !s.ended && s.loadId > 0)
            return MpvSessionFacts(
                playWhenReady = wantsPlay,
                playing = s.playing && state == Player.STATE_READY,
                playbackState = state,
                positionMs = (s.positionSecs * 1000).toLong().coerceAtLeast(0),
                durationMs = (s.durationSecs * 1000).toLong().coerceAtLeast(0),
                error = s.error,
            )
        }
    }
}
