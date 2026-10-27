// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import android.content.Context
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MimeTypes
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionOverride
import androidx.media3.common.Tracks
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import androidx.media3.session.MediaSession
import app.tatakai.next.model.Container
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import okhttp3.OkHttpClient

/** Callbacks of a playback session; all are invoked on the main thread. */
interface PlaybackListener {
    /** Progress tick (every [PlaybackController.PROGRESS_INTERVAL_MS] while playing) and [force]d on pause / stop. */
    fun onProgress(positionMs: Long, durationMs: Long, force: Boolean) = Unit

    /** Heartbeat for remote (Link) sessions, every [PlaybackController.HEARTBEAT_INTERVAL_MS]. */
    fun onHeartbeat(positionMs: Long, paused: Boolean) = Unit
}

/**
 * Media3 ExoPlayer + `OkHttpDataSource` with per-source headers, a MediaSession, automatic fallback to the next source (x3),
 * and the state/callbacks the UI and history need. Create and use it on the main thread; call [release] exactly once.
 */
@UnstableApi
class PlaybackController(
    context: Context,
    private val sources: List<PlayableSource>,
    private val baseClient: OkHttpClient,
    private val scope: CoroutineScope,
    private val listener: PlaybackListener,
    private val remote: Boolean = false,
    startMs: Long = 0,
    startIndex: Int = 0,
) {
    private val appContext = context.applicationContext
    private val fallback = SourceFallback(sources.size, MAX_FALLBACKS, startIndex.coerceIn(0, (sources.size - 1).coerceAtLeast(0)))
    private val _state = MutableStateFlow(PlaybackState(sourceCount = sources.size))
    val state: StateFlow<PlaybackState> = _state.asStateFlow()

    val player: ExoPlayer = ExoPlayer.Builder(appContext)
        .setAudioAttributes(
            AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MOVIE).build(),
            /* handleAudioFocus = */ true,
        )
        .setHandleAudioBecomingNoisy(true)
        .build()

    private val session: MediaSession = MediaSession.Builder(appContext, player).build()
    private var ticker: Job? = null
    private var released = false
    private var resumeAtMs = startMs
    private var lastProgressAt = 0L
    private var lastHeartbeatAt = 0L

    private val playerListener = object : Player.Listener {
        override fun onPlaybackStateChanged(playbackState: Int) {
            if (playbackState == Player.STATE_READY) fallback.onPlaybackStarted()
            publish()
        }

        override fun onIsPlayingChanged(isPlaying: Boolean) {
            if (!isPlaying && !released) listener.onProgress(player.currentPosition, player.duration.orZero(), true)
            publish()
        }

        override fun onTracksChanged(tracks: Tracks) = publish()

        override fun onPlaybackParametersChanged(playbackParameters: androidx.media3.common.PlaybackParameters) = publish()

        override fun onPlayerError(error: PlaybackException) {
            resumeAtMs = player.currentPosition.coerceAtLeast(resumeAtMs)
            val next = fallback.onFailure()
            if (next != null) {
                load(next)
                _state.value = _state.value.copy(fallbackCount = fallback.fallbacksUsed)
            } else {
                _state.value = _state.value.copy(
                    error = PlaybackError(error.errorCode, error.errorCodeName, exhausted = true),
                    isBuffering = false,
                    isPlaying = false,
                )
            }
        }
    }

    init {
        player.addListener(playerListener)
        if (sources.isEmpty()) {
            _state.value = _state.value.copy(isBuffering = false, error = PlaybackError(-1, "NO_SOURCES", exhausted = true))
        } else {
            load(fallback.index)
        }
        ticker = scope.launch {
            while (true) {
                delay(TICK_MS)
                publish()
                if (player.isPlaying) {
                    val now = android.os.SystemClock.elapsedRealtime()
                    if (now - lastProgressAt >= PROGRESS_INTERVAL_MS) {
                        lastProgressAt = now
                        listener.onProgress(player.currentPosition, player.duration.orZero(), false)
                    }
                }
                if (remote) {
                    val now = android.os.SystemClock.elapsedRealtime()
                    if (now - lastHeartbeatAt >= HEARTBEAT_INTERVAL_MS) {
                        lastHeartbeatAt = now
                        listener.onHeartbeat(player.currentPosition, !player.isPlaying)
                    }
                }
            }
        }
    }

    // ---- commands ---------------------------------------------------------------------------------------------------------------

    fun play() = player.play()

    fun pause() = player.pause()

    fun togglePlay() = if (player.isPlaying) pause() else play()

    fun seekTo(positionMs: Long) = player.seekTo(positionMs.coerceIn(0, player.duration.takeIf { it != C.TIME_UNSET } ?: Long.MAX_VALUE))

    fun seekBy(deltaMs: Long) = seekTo(player.currentPosition + deltaMs)

    fun setSpeed(speed: Float) = player.setPlaybackSpeed(speed)

    /** Manual source switch: keeps the position and resets the fallback budget. */
    fun selectSource(index: Int) {
        if (index !in sources.indices) return
        resumeAtMs = player.currentPosition
        fallback.select(index)
        load(index)
    }

    /** Caps the adaptive video height; null = automatic. */
    fun selectVideoHeight(height: Int?) {
        val b = player.trackSelectionParameters.buildUpon()
        if (height == null) b.setMaxVideoSize(Int.MAX_VALUE, Int.MAX_VALUE) else b.setMaxVideoSize(Int.MAX_VALUE, height)
        player.trackSelectionParameters = b.build()
        _state.value = _state.value.copy(selectedHeight = height)
    }

    /** [groupIndex] from [SubtitleOption]; null turns subtitles off. */
    fun selectSubtitle(groupIndex: Int?) {
        val b = player.trackSelectionParameters.buildUpon()
        if (groupIndex == null) {
            b.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, true)
        } else {
            val group = player.currentTracks.groups.getOrNull(groupIndex)?.takeIf { it.type == C.TRACK_TYPE_TEXT } ?: return
            b.setTrackTypeDisabled(C.TRACK_TYPE_TEXT, false)
            b.setOverrideForType(TrackSelectionOverride(group.mediaTrackGroup, 0))
        }
        player.trackSelectionParameters = b.build()
        publish()
    }

    fun release() {
        if (released) return
        listener.onProgress(player.currentPosition, player.duration.orZero(), true)
        released = true
        ticker?.cancel()
        player.removeListener(playerListener)
        session.release()
        player.release()
    }

    // ---- internals --------------------------------------------------------------------------------------------------------------

    private fun load(index: Int) {
        val src = sources[index]
        val client = src.httpClient ?: baseClient
        val factory = OkHttpDataSource.Factory(client).setDefaultRequestProperties(src.headers)
        val subs = src.subtitles.map { t ->
            MediaItem.SubtitleConfiguration.Builder(android.net.Uri.parse(t.url))
                .setMimeType(subtitleMime(t.format))
                .setLanguage(t.lang)
                .setLabel(t.label.ifBlank { t.lang })
                .setSelectionFlags(0)
                .build()
        }
        val item = MediaItem.Builder()
            .setUri(src.url)
            .setMimeType(
                when (src.container) {
                    Container.HLS -> MimeTypes.APPLICATION_M3U8
                    Container.DASH -> MimeTypes.APPLICATION_MPD
                    Container.MP4 -> MimeTypes.VIDEO_MP4
                },
            )
            .setSubtitleConfigurations(subs)
            .build()
        val mediaSource = DefaultMediaSourceFactory(factory).createMediaSource(item)
        player.setMediaSource(mediaSource, resumeAtMs)
        player.prepare()
        player.playWhenReady = true
        _state.value = _state.value.copy(sourceIndex = index, error = null, isBuffering = true, ended = false, videoHeights = emptyList(), subtitles = emptyList())
    }

    private fun subtitleMime(format: String): String =
        when (format.lowercase()) {
            "srt" -> MimeTypes.APPLICATION_SUBRIP
            "ass", "ssa" -> MimeTypes.TEXT_SSA
            else -> MimeTypes.TEXT_VTT
        }

    private fun publish() {
        if (released) return
        val tracks = player.currentTracks
        val heights = tracks.groups.filter { it.type == C.TRACK_TYPE_VIDEO }
            .flatMap { g -> (0 until g.length).filter { g.isTrackSupported(it) }.map { g.getTrackFormat(it).height } }
            .filter { it > 0 }.distinct().sortedDescending()
        val textGroups = tracks.groups.withIndex().filter { it.value.type == C.TRACK_TYPE_TEXT }
        val subtitleOptions = textGroups.map { (i, g) ->
            val f = g.getTrackFormat(0)
            SubtitleOption(i, f.label ?: f.language ?: "#${i + 1}", f.language, g.isSelected)
        }
        val cur = _state.value
        _state.value = cur.copy(
            isPlaying = player.isPlaying,
            isBuffering = player.playbackState == Player.STATE_BUFFERING,
            ended = player.playbackState == Player.STATE_ENDED,
            positionMs = player.currentPosition,
            durationMs = player.duration.orZero(),
            bufferedMs = player.bufferedPosition,
            speed = player.playbackParameters.speed,
            fallbackCount = fallback.fallbacksUsed,
            videoHeights = heights,
            subtitles = subtitleOptions,
            subtitlesEnabled = subtitleOptions.any { it.selected },
        )
    }

    private fun Long.orZero(): Long = if (this == C.TIME_UNSET) 0 else this

    companion object {
        const val MAX_FALLBACKS = 3
        const val TICK_MS = 500L
        const val PROGRESS_INTERVAL_MS = 15_000L
        const val HEARTBEAT_INTERVAL_MS = 4_000L
    }
}
