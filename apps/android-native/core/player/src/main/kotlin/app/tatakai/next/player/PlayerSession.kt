// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.SubtitleStyle
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

// android wave (docs/redesign/12 §4 d.1): engine abstraction of the player. Private to worker d's modules: :core:player implements it
// with Media3 ([PlaybackController]) and :core:player-mpv with libmpv (MpvPlayerSession). The EXTERNAL engine has no session (an Intent
// to another app, see ExternalPlayer.kt).

/**
 * One playback engine instance. Create and drive it on the main thread; [release] exactly once. [state] may be updated from any thread
 * (mpv reports from its event thread) and is safe to collect anywhere.
 */
interface PlayerSession {
    val state: StateFlow<PlaybackState>

    /** Replace whatever plays with [source], starting at [startMs]. Bumps [PlaybackState.loadId] and clears error / started. */
    fun load(source: PlayableSource, startMs: Long)

    fun play()

    fun pause()

    fun togglePlay()

    fun seekTo(positionMs: Long)

    fun seekBy(deltaMs: Long)

    fun setSpeed(speed: Float)

    /** Caps the video height; null = automatic. Engines without adaptive streams may ignore it. */
    fun selectVideoHeight(height: Int?)

    /** [SubtitleOption.groupIndex]; null turns subtitles off. */
    fun selectSubtitle(groupIndex: Int?)

    /** [AudioOption.id]. */
    fun selectAudio(id: Int)

    /** Preferred subtitle / audio languages (ISO 639-1, `SourcePrefs.subLang` / `dubLang`) for the next tracks the engine picks. */
    fun setPreferredLanguages(subtitle: String?, audio: String?)

    /** Settings › Reproductor › Estilo de subtítulos. ASS keeps its own styles where the engine renders them. */
    fun setSubtitleStyle(style: SubtitleStyle)

    /** Current position right now (the [state] tick may lag up to half a second). */
    fun currentPositionMs(): Long

    fun release()
}

/**
 * Progress / heartbeat ticks shared by the engines: [PlaybackListener.onProgress] every [PLAYER_PROGRESS_INTERVAL_MS] while
 * playing, and [PlaybackListener.onHeartbeat] every [PLAYER_HEARTBEAT_INTERVAL_MS] for remote sessions. Forced ticks on pause /
 * release are the engine's job.
 */
class SessionTicker(
    private val scope: CoroutineScope,
    private val listener: PlaybackListener,
    private val remote: Boolean,
    private val now: () -> Long,
    private val snapshot: () -> Triple<Long, Long, Boolean>,
    private val onTick: () -> Unit = {},
) {
    private var job: Job? = null
    private var lastProgressAt = 0L
    private var lastHeartbeatAt = 0L

    fun start() {
        if (job != null) return
        job = scope.launch {
            while (true) {
                delay(PLAYER_TICK_MS)
                onTick()
                val (pos, dur, playing) = snapshot()
                val t = now()
                if (playing && t - lastProgressAt >= PLAYER_PROGRESS_INTERVAL_MS) {
                    lastProgressAt = t
                    listener.onProgress(pos, dur, false)
                }
                if (remote && t - lastHeartbeatAt >= PLAYER_HEARTBEAT_INTERVAL_MS) {
                    lastHeartbeatAt = t
                    listener.onHeartbeat(pos, !playing)
                }
            }
        }
    }

    fun stop() {
        job?.cancel()
        job = null
    }
}

/** Engine state refresh tick. */
const val PLAYER_TICK_MS: Long = 500L

/** History progress row while playing. */
const val PLAYER_PROGRESS_INTERVAL_MS: Long = 15_000L

/** Link (remote) heartbeat. */
const val PLAYER_HEARTBEAT_INTERVAL_MS: Long = 4_000L
