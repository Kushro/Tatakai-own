// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.BatteryManager
import android.os.Looper
import android.os.SystemClock
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.framegen.FgController
import app.tatakai.next.framegen.FrameGenController
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.player.ActivePlayback
import app.tatakai.next.player.AudioOption
import app.tatakai.next.player.PlayableSource
import app.tatakai.next.player.PlaybackError
import app.tatakai.next.player.PlaybackListener
import app.tatakai.next.player.PlaybackState
import app.tatakai.next.player.PlayerSession
import app.tatakai.next.player.PlayerSessionFactory
import app.tatakai.next.player.SessionTicker
import app.tatakai.next.player.MPV_OPEN_ERROR_CODE
import app.tatakai.next.player.MPV_START_TIMEOUT_CODE
import app.tatakai.next.player.MPV_START_TIMEOUT_MS
import app.tatakai.next.player.ShaderFallback
import app.tatakai.next.player.ShaderNotice
import app.tatakai.next.player.SubtitleExtras
import app.tatakai.next.player.SubtitleFonts
import app.tatakai.next.player.SubtitleOption
import app.tatakai.next.player.mpvStartWatched
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.withContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
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
@UnstableApi
class MpvPlayerSession(
    context: Context,
    private val scope: CoroutineScope,
    private val listener: PlaybackListener,
    remote: Boolean,
    /** Settings › Reproductor › Preajuste (only applied when its shader files exist). */
    private val preset: String = "off",
    /** 0.4.0 (worker B): the app-wide phone frame generation controller (null = FG not wired, e.g. tests). */
    frameGenController: FgController? = null,
) : PlayerSession {
    val engine = MpvEngine(context)

    /** 0.4.0 (worker B): routes this engine's output through the phone FG (see MpvFrameGenBridge); released before the engine. */
    private val frameGenBridge: MpvFrameGenBridge? = frameGenController?.let { MpvFrameGenBridge(engine, it) }

    /** 0.4.0 (worker B): phone frame generation of this playback for the in-player UI; null when FG is not wired. */
    val frameGen: FrameGenController? = frameGenController
    private val _state = MutableStateFlow(PlaybackState(isBuffering = false, fullAss = true))
    override val state: StateFlow<PlaybackState> = _state.asStateFlow()
    private var released = false
    private var loaded = false
    private var wasPlaying = false
    private val mirror: Job
    private val fonts = SubtitleFonts(context)
    private var style = SubtitleStyle()
    private var extras: SubtitleExtras? = null

    /** parity wave A (15 §3.2): the engine as a Media3 Player for PlaybackService (background, notification, headset buttons). */
    val adapter: MpvPlayerAdapter = MpvPlayerAdapter(engine, Looper.getMainLooper())

    private val ticker = SessionTicker(
        scope = scope,
        listener = listener,
        remote = remote,
        now = { SystemClock.elapsedRealtime() },
        snapshot = { val s = _state.value; Triple(currentPositionMs(), s.durationMs, s.isPlaying) },
    )

    /** BUG-043 / BUG-006: battery rule and real-time step-down of the shader chain. */
    private val governor = PresetGovernor(preset)
    private val appContext = context.applicationContext
    private var noticeSeq = 0
    private var governorJob: Job? = null
    private var startWatch: Job? = null
    private var prepareJob: Job? = null
    private var audioLang: String? = null
    private var subLang: String? = null

    /** loadId of the last load watched by the start timeout ([mpvStartWatched]); its errors before a frame go to Media3 (BUG-080). */
    @Volatile private var watchedLoadId = -1

    private fun battery(): BatteryInfo? = runCatching {
        val intent = appContext.registerReceiver(null, IntentFilter(Intent.ACTION_BATTERY_CHANGED)) ?: return@runCatching null
        val level = intent.getIntExtra(BatteryManager.EXTRA_LEVEL, -1)
        val scale = intent.getIntExtra(BatteryManager.EXTRA_SCALE, -1)
        if (level < 0 || scale <= 0) return@runCatching null
        val status = intent.getIntExtra(BatteryManager.EXTRA_STATUS, -1)
        val charging = status == BatteryManager.BATTERY_STATUS_CHARGING || status == BatteryManager.BATTERY_STATUS_FULL
        BatteryInfo(level * 100 / scale, charging)
    }.getOrNull()

    private fun announce(kind: ShaderFallback) {
        noticeSeq++
        _state.value = _state.value.copy(shaderNotice = ShaderNotice(noticeSeq, kind))
        android.util.Log.i("mpv", "shader preset -> ${governor.preset} ($kind)")
    }

    private fun startGovernor() {
        governorJob = scope.launch {
            var sinceBattery = 0L
            while (isActive && !released) {
                delay(PresetGovernor.SAMPLE_MS)
                if (released || ShaderPresets.chain(governor.preset).isEmpty()) break
                sinceBattery += PresetGovernor.SAMPLE_MS
                if (sinceBattery >= BATTERY_CHECK_MS) {
                    sinceBattery = 0
                    governor.onBattery(battery())?.let { change ->
                        engine.applyPreset(governor.preset)
                        announce(change)
                    }
                }
                val st = _state.value
                if (!st.isPlaying || st.isBuffering) {
                    governor.reset()
                    continue
                }
                val drops = engine.droppedFrames() ?: continue
                val fps = engine.videoFps() ?: continue
                governor.onSample(drops, fps, SystemClock.elapsedRealtime())?.let { change ->
                    engine.applyPreset(governor.preset)
                    announce(change)
                }
            }
        }
    }

    init {
        val (initialPreset, change) = governor.initial(battery())
        engine.applyPreset(initialPreset)
        change?.let { announce(it) }
        if (ShaderPresets.chain(initialPreset).isNotEmpty()) startGovernor()
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
                    error = m.error?.let { PlaybackError(mpvErrorCode(it, m.started, m.loadId == watchedLoadId), it, exhausted = true) },
                    subText = m.subText,
                    subtitleDelayMs = m.subDelayMs,
                    loop = m.loop,
                    secondarySubtitleId = m.secondarySid,
                )
                if (wasPlaying && !m.playing && loaded && !released) listener.onProgress(currentPositionMs(), _state.value.durationMs, true)
                wasPlaying = m.playing
            }
        }
        ticker.start()
        ActivePlayback.publish(adapter)
    }

    override fun load(source: PlayableSource, startMs: Long) {
        if (released) return
        loaded = true
        ActivePlayback.publish(adapter)
        governor.reset()
        startWatch?.cancel()
        prepareJob?.cancel()
        val loadId = engine.beginLoad()
        val startSecs = startMs.coerceAtLeast(0) / 1000.0
        val subs = source.subtitles.map { it.url }
        if (!HlsPrefetch.applies(source)) {
            open(source, source.url, startSecs, subs, loadId)
            return
        }
        // BUG-059: FFmpeg waits for every rendition of an HLS master; hand mpv a master with only the variant and the languages it
        // will play. The PC's extension-server sources (no headers needed) are the ones with 15 audio renditions.
        prepareJob = scope.launch {
            val url = withContext(Dispatchers.IO) { HlsPrefetch.filteredMaster(appContext, source.url, audioLang, subLang) } ?: source.url
            if (!released && engine.state.value.loadId == loadId) open(source, url, startSecs, subs, loadId)
        }
    }

    private fun open(source: PlayableSource, url: String, startSecs: Double, subs: List<String>, loadId: Int) {
        val lavf = HlsPrefetch.lavfOptions(url, source.url)
        if (lavf.isNotEmpty()) android.util.Log.i("mpv", "filtered HLS master opened with demuxer-lavf-o=$lavf")
        watchedLoadId = if (mpvStartWatched(source.container, source.torrent)) loadId else -1
        engine.loadPrepared(url, source.headers, startSecs, subs, lavf)
        // BUG-059: a network stream that shows no frame in time is given up (the view-model moves it to Media3). Safety net only:
        // the filtered master above is what makes such streams start.
        startWatch = if (mpvStartWatched(source.container, source.torrent)) {
            scope.launch {
                delay(MPV_START_TIMEOUT_MS)
                val m = engine.state.value
                if (!released && m.loadId == loadId && !m.started && m.error == null) {
                    android.util.Log.w("mpv", "no frame after ${MPV_START_TIMEOUT_MS / 1000} s: giving up on this load")
                    engine.failStart(START_TIMEOUT)
                }
            }
        } else {
            null
        }
    }

    override fun play() = engine.play()

    override fun pause() = engine.pause()

    override fun togglePlay() = if (_state.value.isPlaying) pause() else play()

    override fun seekTo(positionMs: Long) {
        governor.reset()
        engine.seekTo(positionMs.coerceAtLeast(0) / 1000.0)
    }

    override fun seekBy(deltaMs: Long) = seekTo(currentPositionMs() + deltaMs)

    override fun setSpeed(speed: Float) = engine.setSpeed(speed.toDouble())

    /** mpv picks the stream quality itself; adaptive height caps are not offered on this engine (no video heights in the state). */
    override fun selectVideoHeight(height: Int?) = Unit

    override fun selectSubtitle(groupIndex: Int?) = engine.selectSubtitle(groupIndex)

    override fun selectAudio(id: Int) = engine.selectAudio(id)

    override fun setPreferredLanguages(subtitle: String?, audio: String?) {
        subLang = subtitle
        audioLang = audio
        engine.setPreferredLanguages(subtitle, audio)
    }

    override fun setSubtitleStyle(style: SubtitleStyle) {
        this.style = style
        engine.setSubtitleStyle(style.scale, style.backgroundOpacity, style.bold, style.outline)
        extras?.let { applyExtras(it) }
    }

    override fun setSubtitleExtras(extras: SubtitleExtras) {
        this.extras = extras
        applyExtras(extras)
    }

    private fun applyExtras(e: SubtitleExtras) {
        val family = e.fontFile?.let { fonts.familyName(it) }
        engine.setSubtitleExtras(e, style.outline, family)
    }

    override fun setSubtitleDelay(ms: Long) = engine.setSubtitleDelay(ms)

    override fun addSubtitleFile(path: String, title: String): Boolean {
        engine.addSubtitleFile(path, title)
        return true
    }

    override val supportsSecondaryTracks: Boolean get() = true

    override fun selectSecondarySubtitle(id: Int?): Boolean {
        engine.selectSecondarySubtitle(id)
        return true
    }

    override fun setSubtitlesVisible(visible: Boolean) = engine.setSubtitlesVisible(visible)

    override fun setLoop(loop: Boolean) = engine.setLoop(loop)

    override fun setStableVolume(enabled: Boolean) = engine.setStableVolume(enabled)

    override fun screenshotToFile(path: String): Boolean = engine.screenshotToFile(path)

    override fun currentPositionMs(): Long = (engine.currentPositionSecs() * 1000).toLong()

    override fun release() {
        if (released) return
        if (loaded) listener.onProgress(currentPositionMs(), _state.value.durationMs, true)
        released = true
        ticker.stop()
        governorJob?.cancel()
        startWatch?.cancel()
        prepareJob?.cancel()
        mirror.cancel()
        if (ActivePlayback.player.value === adapter) ActivePlayback.publish(null)
        adapter.detach()
        frameGenBridge?.release() // 0.4.0 (worker B): FG off and unbound before MPVLib.destroy
        engine.release()
    }
}

private const val BATTERY_CHECK_MS = 60_000L

/** mpv error text of a load given up by the start watchdog (BUG-059). */
internal const val START_TIMEOUT = "start timeout"

/**
 * [PlaybackError.code] for an mpv error: the start timeout, an error before the first frame of a watched network stream (BUG-080:
 * the view-model moves both to Media3), or -1 (the source fails as usual: torrents, local files, errors after playback started).
 */
internal fun mpvErrorCode(error: String, started: Boolean, watched: Boolean): Int = when {
    error == START_TIMEOUT -> MPV_START_TIMEOUT_CODE
    !started && watched -> MPV_OPEN_ERROR_CODE
    else -> -1
}

/** Koin-bound factory of mpv sessions (see `mpvPlayerModule`). [preset] is read at creation time. */
@UnstableApi
class MpvSessionFactory(
    private val context: Context,
    private val preset: () -> String,
    // 0.4.0 (worker B): phone frame generation (null = not wired).
    private val frameGen: FgController? = null,
) : PlayerSessionFactory {
    override fun create(scope: CoroutineScope, listener: PlaybackListener, remote: Boolean): PlayerSession =
        MpvPlayerSession(context, scope, listener, remote, preset(), frameGen)
}
