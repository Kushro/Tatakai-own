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
import androidx.media3.common.text.Cue
import androidx.media3.common.text.CueGroup
import androidx.media3.common.util.UnstableApi
import androidx.media3.datasource.okhttp.OkHttpDataSource
import androidx.media3.exoplayer.DefaultLoadControl
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.source.DefaultMediaSourceFactory
import android.media.audiofx.AudioEffect
import android.media.audiofx.DynamicsProcessing
import android.media.audiofx.LoudnessEnhancer
import android.os.Build
import androidx.media3.common.MediaMetadata
import app.tatakai.next.model.Container
import app.tatakai.next.model.SubtitleStyle
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.OkHttpClient
import java.util.concurrent.TimeUnit

/** Callbacks of a playback session; all are invoked on the main thread. */
interface PlaybackListener {
    /** Progress tick (every [PLAYER_PROGRESS_INTERVAL_MS] while playing) and [force]d on pause / stop. */
    fun onProgress(positionMs: Long, durationMs: Long, force: Boolean) = Unit

    /** Heartbeat for remote (Link) sessions, every [PLAYER_HEARTBEAT_INTERVAL_MS]. */
    fun onHeartbeat(positionMs: Long, paused: Boolean) = Unit
}

/**
 * Media3 engine ([PlayerSession]): ExoPlayer + `OkHttpDataSource` with per-source headers, published to [ActivePlayback] (the one
 * `MediaSession` lives in [PlaybackService], parity wave A) and the state / callbacks the
 * UI and history need. Failover between sources is the caller's job (android wave: `SourceFlow` in the player view-model); a failed load
 * only sets [PlaybackState.error]. Torrent loopback streams (`Container.PROGRESSIVE`) get long read timeouts; the load control keeps a
 * 30-60 s buffer for everything (11 §5.3). Subtitles are NOT drawn by `ContentFrame`: [cues] carries them and the screen renders them
 * with the user's [subtitleStyle]. Create and use it on the main thread; call [release] exactly once.
 */
@UnstableApi
class PlaybackController(
    context: Context,
    private val baseClient: OkHttpClient,
    private val scope: CoroutineScope,
    private val listener: PlaybackListener,
    private val remote: Boolean = false,
) : PlayerSession {
    private val appContext = context.applicationContext
    private val _state = MutableStateFlow(PlaybackState(isBuffering = false))
    override val state: StateFlow<PlaybackState> = _state.asStateFlow()

    private val _cues = MutableStateFlow<List<Cue>>(emptyList())

    /** Current subtitle cues (Media3 parses SRT / VTT / SSA into plain cues; ASS styling is lost). */
    val cues: StateFlow<List<Cue>> = _cues.asStateFlow()

    private val _subtitleStyle = MutableStateFlow(SubtitleStyle())
    val subtitleStyle: StateFlow<SubtitleStyle> = _subtitleStyle.asStateFlow()

    /** Long timeouts for loopback torrent reads: the server blocks until the piece arrives (11 §5.3). */
    private val progressiveClient: OkHttpClient by lazy {
        baseClient.newBuilder()
            .connectTimeout(TORRENT_CONNECT_TIMEOUT_S, TimeUnit.SECONDS)
            .readTimeout(TORRENT_READ_TIMEOUT_S, TimeUnit.SECONDS)
            .callTimeout(0, TimeUnit.SECONDS)
            .build()
    }

    val player: ExoPlayer = ExoPlayer.Builder(appContext)
        .setAudioAttributes(
            AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_MOVIE).build(),
            /* handleAudioFocus = */ true,
        )
        .setHandleAudioBecomingNoisy(true)
        .setLoadControl(
            DefaultLoadControl.Builder()
                .setBufferDurationsMs(MIN_BUFFER_MS, MAX_BUFFER_MS, BUFFER_FOR_PLAYBACK_MS, BUFFER_AFTER_REBUFFER_MS)
                .setPrioritizeTimeOverSizeThresholds(true)
                .build(),
        )
        .build()

    private var released = false
    private var loaded = false

    // --- parity wave A (worker p) ---
    /** ISO 639-1 subtitle preference (`SourcePrefs.subLang`), for the forced selection of [onTracksChanged]. */
    private var preferredSub: String? = null

    /** loadId whose subtitle auto-selection was already decided (forced, or the user chose). */
    private var subDecidedLoad = -1
    private var loudness: AudioEffect? = null
    private var stableVolume = false

    private val ticker = SessionTicker(
        scope = scope,
        listener = listener,
        remote = remote,
        now = { android.os.SystemClock.elapsedRealtime() },
        snapshot = { Triple(player.currentPosition, player.duration.orZero(), player.isPlaying) },
        onTick = { publish() },
    )

    private val playerListener = object : Player.Listener {
        override fun onPlaybackStateChanged(playbackState: Int) {
            if (playbackState == Player.STATE_READY && loaded) _state.value = _state.value.copy(started = true)
            publish()
        }

        override fun onIsPlayingChanged(isPlaying: Boolean) {
            if (!isPlaying && !released && loaded) listener.onProgress(player.currentPosition, player.duration.orZero(), true)
            publish()
        }

        override fun onTracksChanged(tracks: Tracks) {
            ensurePreferredSubtitle(tracks)
            publish()
        }

        override fun onPlaybackParametersChanged(playbackParameters: androidx.media3.common.PlaybackParameters) = publish()

        override fun onCues(cueGroup: CueGroup) {
            _cues.value = cueGroup.cues
        }

        override fun onPlayerError(error: PlaybackException) {
            _state.value = _state.value.copy(
                error = PlaybackError(error.errorCode, error.errorCodeName, exhausted = true),
                isBuffering = false,
                isPlaying = false,
            )
        }
    }

    init {
        player.addListener(playerListener)
        ticker.start()
        // parity wave A (15 §3.2): background playback / the system media controls go through PlaybackService.
        ActivePlayback.publish(player)
    }

    // ---- commands ---------------------------------------------------------------------------------------------------------------

    override fun load(source: PlayableSource, startMs: Long) {
        if (released) return
        val client = source.httpClient ?: if (source.torrent || source.container == Container.PROGRESSIVE) progressiveClient else baseClient
        val factory = OkHttpDataSource.Factory(client).setDefaultRequestProperties(source.headers)
        // Bug player.subs.preferred-lang: extension tracks say `es-419`, `spa`, `Spanish`...; normalised to ISO 639-1 so the preferred
        // language matches. The id carries the index of the external track (SubtitleOption.external).
        val subs = source.subtitles.mapIndexed { i, t ->
            MediaItem.SubtitleConfiguration.Builder(android.net.Uri.parse(t.url))
                .setMimeType(subtitleMime(t.format))
                .setLanguage(LanguageCodes.normalize(t.lang) ?: LanguageCodes.normalize(t.label) ?: t.lang.ifBlank { null })
                .setLabel(t.label.ifBlank { t.lang })
                .setId("$EXTERNAL_ID_PREFIX$i")
                .setSelectionFlags(0)
                .build()
        }
        val meta = ActivePlayback.metadata.value
        val item = MediaItem.Builder()
            .setUri(source.url)
            .setMimeType(
                when (source.container) {
                    Container.HLS -> MimeTypes.APPLICATION_M3U8
                    Container.DASH -> MimeTypes.APPLICATION_MPD
                    Container.MP4 -> MimeTypes.VIDEO_MP4
                    // Torrent loopback file (MKV/MP4/WebM) or a completed download: Media3 sniffs the extractor.
                    Container.PROGRESSIVE -> null
                },
            )
            .setSubtitleConfigurations(subs)
            .apply {
                if (meta != null) {
                    setMediaMetadata(
                        MediaMetadata.Builder()
                            .setTitle(meta.title)
                            .setArtist(meta.subtitle)
                            .setDisplayTitle(meta.title)
                            .apply { meta.artworkUri?.let { setArtworkUri(android.net.Uri.parse(it)) } }
                            .build(),
                    )
                }
            }
            .build()
        val mediaSource = DefaultMediaSourceFactory(factory)
            .apply { if (remote) setLoadErrorHandlingPolicy(RemoteLoadErrorPolicy()) }
            .createMediaSource(item)
        loaded = true
        _cues.value = emptyList()
        _state.value = _state.value.copy(
            loadId = _state.value.loadId + 1,
            started = false,
            error = null,
            isBuffering = true,
            ended = false,
            videoHeights = emptyList(),
            subtitles = emptyList(),
            audioTracks = emptyList(),
            subtitleIsAss = false,
        )
        ActivePlayback.publish(player)
        player.setMediaSource(mediaSource, startMs.coerceAtLeast(0))
        player.prepare()
        player.playWhenReady = true
    }

    override fun play() = player.play()

    override fun pause() = player.pause()

    override fun togglePlay() = if (player.isPlaying) pause() else play()

    override fun seekTo(positionMs: Long) =
        player.seekTo(positionMs.coerceIn(0, player.duration.takeIf { it != C.TIME_UNSET } ?: Long.MAX_VALUE))

    override fun seekBy(deltaMs: Long) = seekTo(player.currentPosition + deltaMs)

    override fun setSpeed(speed: Float) = player.setPlaybackSpeed(speed)

    override fun currentPositionMs(): Long = player.currentPosition

    /** Caps the adaptive video height; null = automatic. */
    override fun selectVideoHeight(height: Int?) {
        val b = player.trackSelectionParameters.buildUpon()
        if (height == null) b.setMaxVideoSize(Int.MAX_VALUE, Int.MAX_VALUE) else b.setMaxVideoSize(Int.MAX_VALUE, height)
        player.trackSelectionParameters = b.build()
        _state.value = _state.value.copy(selectedHeight = height)
    }

    /** [groupIndex] from [SubtitleOption]; null turns subtitles off. */
    override fun selectSubtitle(groupIndex: Int?) {
        subDecidedLoad = _state.value.loadId
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

    override fun selectAudio(id: Int) {
        val group = player.currentTracks.groups.getOrNull(id)?.takeIf { it.type == C.TRACK_TYPE_AUDIO } ?: return
        player.trackSelectionParameters = player.trackSelectionParameters.buildUpon()
            .setTrackTypeDisabled(C.TRACK_TYPE_AUDIO, false)
            .setOverrideForType(TrackSelectionOverride(group.mediaTrackGroup, 0))
            .build()
        publish()
    }

    /**
     * Bug player.subs.preferred-lang: Media3 compares raw tags, so `es` never matched `es-419` / `spa`. Every variant of the language is
     * handed over ([LanguageCodes.engineVariants]) and [ensurePreferredSubtitle] forces a matching track when Media3 still picked none.
     */
    override fun setPreferredLanguages(subtitle: String?, audio: String?) {
        preferredSub = LanguageCodes.normalize(subtitle)
        val text = LanguageCodes.engineVariants(subtitle).toTypedArray()
        val sound = LanguageCodes.engineVariants(audio).toTypedArray()
        player.trackSelectionParameters = player.trackSelectionParameters.buildUpon()
            .setPreferredTextLanguages(*text)
            .setPreferredAudioLanguages(*sound)
            .build()
    }

    /** First tracks of a load with no subtitle selected although one is in the preferred language: select it (once per load). */
    private fun ensurePreferredSubtitle(tracks: Tracks) {
        val loadId = _state.value.loadId
        if (!loaded || subDecidedLoad == loadId) return
        val text = tracks.groups.withIndex().filter { it.value.type == C.TRACK_TYPE_TEXT }
        if (text.isEmpty()) return
        subDecidedLoad = loadId
        if (text.any { it.value.isSelected }) return
        val pick = LanguageCodes.firstMatch(text, preferredSub, { it.value.getTrackFormat(0).language }, { it.value.getTrackFormat(0).label }) ?: return
        selectSubtitle(text[pick].index)
    }

    override fun setSubtitleDelay(ms: Long) {
        _state.value = _state.value.copy(subtitleDelayMs = SubtitleDelay.clamp(ms))
    }

    override fun setLoop(loop: Boolean) {
        player.repeatMode = if (loop) Player.REPEAT_MODE_ONE else Player.REPEAT_MODE_OFF
        _state.value = _state.value.copy(loop = loop)
    }

    /** Stable volume: a limiter with make-up gain on the player's audio session (API 28+), else a mild LoudnessEnhancer. */
    override fun setStableVolume(enabled: Boolean) {
        stableVolume = enabled
        loudness?.let { runCatching { it.release() } }
        loudness = null
        if (!enabled || released) return
        val sessionId = player.audioSessionId.takeIf { it != C.AUDIO_SESSION_ID_UNSET } ?: return
        loudness = runCatching {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                val config = DynamicsProcessing.Config.Builder(
                    DynamicsProcessing.VARIANT_FAVOR_FREQUENCY_RESOLUTION, 2, false, 0, false, 0, false, 0, true,
                ).build()
                DynamicsProcessing(0, sessionId, config).apply {
                    setInputGainAllChannelsTo(STABLE_INPUT_GAIN_DB)
                    setLimiterAllChannelsTo(DynamicsProcessing.Limiter(true, true, 0, 1f, 60f, 10f, -12f, 0f))
                    setEnabled(true)
                }
            } else {
                LoudnessEnhancer(sessionId).apply {
                    setTargetGain(STABLE_GAIN_MB)
                    setEnabled(true)
                }
            }
        }.getOrNull()
    }

    override fun setSubtitleStyle(style: SubtitleStyle) {
        _subtitleStyle.value = style
    }

    override fun release() {
        if (released) return
        if (loaded) listener.onProgress(player.currentPosition, player.duration.orZero(), true)
        released = true
        ticker.stop()
        player.removeListener(playerListener)
        if (ActivePlayback.player.value === player) ActivePlayback.publish(null)
        loudness?.let { runCatching { it.release() } }
        loudness = null
        player.release()
    }

    // ---- internals --------------------------------------------------------------------------------------------------------------

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
            // Media3 >= 1.4 parses subtitles during extraction: the original mime survives in `codecs`.
            val ass = f.sampleMimeType == MimeTypes.TEXT_SSA || f.codecs == MimeTypes.TEXT_SSA
            val external = f.id?.substringAfter(EXTERNAL_ID_PREFIX, "")?.takeWhile { it.isDigit() }?.toIntOrNull()
            SubtitleOption(i, f.label ?: f.language ?: "#${i + 1}", f.language, g.isSelected, ass, external)
        }
        val audioOptions = tracks.groups.withIndex().filter { it.value.type == C.TRACK_TYPE_AUDIO }.map { (i, g) ->
            val f = g.getTrackFormat(0)
            val channels = f.channelCount.takeIf { it > 0 }?.let { " · ${it}ch" }.orEmpty()
            AudioOption(i, (f.label ?: f.language ?: "#${i + 1}") + channels, f.language, g.isSelected)
        }
        val cur = _state.value
        _state.value = cur.copy(
            isPlaying = player.isPlaying,
            isBuffering = loaded && player.playbackState == Player.STATE_BUFFERING,
            ended = player.playbackState == Player.STATE_ENDED,
            positionMs = player.currentPosition,
            durationMs = player.duration.orZero(),
            bufferedMs = player.bufferedPosition,
            speed = player.playbackParameters.speed,
            videoHeights = heights,
            subtitles = subtitleOptions,
            subtitlesEnabled = subtitleOptions.any { it.selected },
            subtitleIsAss = subtitleOptions.any { it.selected && it.ass },
            audioTracks = audioOptions,
            fullAss = false,
        )
    }

    private fun Long.orZero(): Long = if (this == C.TIME_UNSET) 0 else this

    companion object {
        const val MAX_FALLBACKS = 3
        const val TICK_MS = PLAYER_TICK_MS
        const val PROGRESS_INTERVAL_MS = PLAYER_PROGRESS_INTERVAL_MS
        const val HEARTBEAT_INTERVAL_MS = PLAYER_HEARTBEAT_INTERVAL_MS

        // 11 §5.3: 30-60 s buffer, prioritise time over size (torrent loopback streams start slowly and stall on missing pieces).
        const val MIN_BUFFER_MS = 30_000
        const val MAX_BUFFER_MS = 60_000
        const val BUFFER_FOR_PLAYBACK_MS = 2_500
        const val BUFFER_AFTER_REBUFFER_MS = 5_000
        const val TORRENT_CONNECT_TIMEOUT_S = 30L
        const val TORRENT_READ_TIMEOUT_S = 90L

        /** SubtitleConfiguration id of external track N (`ext-N`). */
        const val EXTERNAL_ID_PREFIX = "ext-"
        const val STABLE_INPUT_GAIN_DB = 6f
        const val STABLE_GAIN_MB = 400
    }
}
