// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.content.Context
import android.os.SystemClock
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.player.AudioOption
import app.tatakai.next.player.PlayableSource
import app.tatakai.next.player.PlaybackError
import app.tatakai.next.player.PlaybackListener
import app.tatakai.next.player.PlaybackState
import app.tatakai.next.player.PlayerSession
import app.tatakai.next.player.PlayerSessionFactory
import app.tatakai.next.player.SessionTicker
import app.tatakai.next.player.SubtitleOption
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** mpv codec names that libass renders with full styling. */
private val ASS_CODECS = setOf("ass", "ssa")

/**
 * [PlayerSession] over one [MpvEngine]: maps [MpvState] to the shared [PlaybackState] (so feature/player draws the same controls for
 * every engine) and reports progress like the Media3 engine. Create on the main thread; [release] exactly once.
 */
class MpvPlayerSession(
    context: Context,
    private val scope: CoroutineScope,
    private val listener: PlaybackListener,
    remote: Boolean,
    /** Settings › Reproductor › Preajuste (only applied when its shader files exist). */
    private val preset: String = "off",
) : PlayerSession {
    val engine = MpvEngine(context)
    private val _state = MutableStateFlow(PlaybackState(isBuffering = false, fullAss = true))
    override val state: StateFlow<PlaybackState> = _state.asStateFlow()
    private var released = false
    private var loaded = false
    private var wasPlaying = false
    private val mirror: Job

    private val ticker = SessionTicker(
        scope = scope,
        listener = listener,
        remote = remote,
        now = { SystemClock.elapsedRealtime() },
        snapshot = { val s = _state.value; Triple(currentPositionMs(), s.durationMs, s.isPlaying) },
    )

    init {
        engine.applyPreset(preset)
        mirror = scope.launch {
            engine.state.collect { m ->
                val prev = _state.value
                val subs = m.subtitles.map { t ->
                    SubtitleOption(t.id, t.title ?: t.lang ?: "#${t.id}", t.lang, t.selected, ass = t.codec?.lowercase() in ASS_CODECS)
                }
                _state.value = prev.copy(
                    loadId = m.loadId,
                    started = m.started,
                    isPlaying = m.playing,
                    isBuffering = loaded && (m.buffering && m.error == null),
                    ended = m.ended,
                    positionMs = (m.positionSecs * 1000).toLong(),
                    durationMs = (m.durationSecs * 1000).toLong(),
                    bufferedMs = ((m.positionSecs + m.cacheSecs) * 1000).toLong(),
                    speed = m.speed.toFloat(),
                    videoHeights = emptyList(),
                    subtitles = subs,
                    subtitlesEnabled = subs.any { it.selected },
                    subtitleIsAss = subs.any { it.selected && it.ass },
                    audioTracks = m.audio.map { t -> AudioOption(t.id, t.title ?: t.lang ?: "#${t.id}", t.lang, t.selected) },
                    fullAss = true,
                    error = m.error?.let { PlaybackError(-1, it, exhausted = true) },
                )
                if (wasPlaying && !m.playing && loaded && !released) listener.onProgress(currentPositionMs(), _state.value.durationMs, true)
                wasPlaying = m.playing
            }
        }
        ticker.start()
    }

    override fun load(source: PlayableSource, startMs: Long) {
        if (released) return
        loaded = true
        engine.load(source.url, source.headers, startMs.coerceAtLeast(0) / 1000.0, source.subtitles.map { it.url })
    }

    override fun play() = engine.play()

    override fun pause() = engine.pause()

    override fun togglePlay() = if (_state.value.isPlaying) pause() else play()

    override fun seekTo(positionMs: Long) = engine.seekTo(positionMs.coerceAtLeast(0) / 1000.0)

    override fun seekBy(deltaMs: Long) = seekTo(currentPositionMs() + deltaMs)

    override fun setSpeed(speed: Float) = engine.setSpeed(speed.toDouble())

    /** mpv picks the stream quality itself; adaptive height caps are not offered on this engine (no video heights in the state). */
    override fun selectVideoHeight(height: Int?) = Unit

    override fun selectSubtitle(groupIndex: Int?) = engine.selectSubtitle(groupIndex)

    override fun selectAudio(id: Int) = engine.selectAudio(id)

    override fun setPreferredLanguages(subtitle: String?, audio: String?) = engine.setPreferredLanguages(subtitle, audio)

    override fun setSubtitleStyle(style: SubtitleStyle) =
        engine.setSubtitleStyle(style.scale, style.backgroundOpacity, style.bold, style.outline)

    override fun currentPositionMs(): Long = (engine.currentPositionSecs() * 1000).toLong()

    override fun release() {
        if (released) return
        if (loaded) listener.onProgress(currentPositionMs(), _state.value.durationMs, true)
        released = true
        ticker.stop()
        mirror.cancel()
        engine.release()
    }
}

/** Koin-bound factory of mpv sessions (see `mpvPlayerModule`). [preset] is read at creation time. */
class MpvSessionFactory(private val context: Context, private val preset: () -> String) : PlayerSessionFactory {
    override fun create(scope: CoroutineScope, listener: PlaybackListener, remote: Boolean): PlayerSession =
        MpvPlayerSession(context, scope, listener, remote, preset())
}
