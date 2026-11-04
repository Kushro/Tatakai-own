// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.content.Context
import android.view.Surface
import dev.jdtech.mpv.MPVLib
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import java.io.File
import java.util.Locale

// android wave contract (docs/redesign/12 §3.2). Owner: worker d. Only the names below are fixed for other workers (the app lists
// [mpvPlayerModule]; feature/player renders [MpvPlayerSurface]); everything else in this module is d's to shape.
// libmpv API: dev.jdtech.mpv.MPVLib instance API (MPVLib.create(context), init(), destroy(), attachSurface(Surface), detachSurface(),
// command(Array<String>), setOptionString, get/setProperty{Int,Double,Boolean,String}, observeProperty(name, MpvFormat), addObserver,
// addLogObserver) - see .deps-sources/libmpv-1.0.0/MPVLib.signatures.txt.

/** One subtitle / audio track as mpv lists it (`track-list/N/...`). [id] is mpv's `sid` / `aid`. */
data class MpvTrack(val id: Int, val title: String?, val lang: String?, val codec: String?, val selected: Boolean)

/** Minimal state the player UI renders for the mpv branch (d may extend it). */
data class MpvState(
    val playing: Boolean = false,
    val buffering: Boolean = true,
    val ended: Boolean = false,
    val positionSecs: Double = 0.0,
    val durationSecs: Double = 0.0,
    val error: String? = null,
    // --- worker d extensions ---
    /** Bumped by every [MpvEngine.load]. */
    val loadId: Int = 0,
    /** The current file played at least once (first `playback-restart` after loading). */
    val started: Boolean = false,
    val speed: Double = 1.0,
    /** Seconds buffered ahead (`demuxer-cache-time`). */
    val cacheSecs: Double = 0.0,
    val subtitles: List<MpvTrack> = emptyList(),
    val audio: List<MpvTrack> = emptyList(),
    /** Video height of the current file (`height`), 0 = unknown. */
    val videoHeight: Int = 0,
)

/**
 * One libmpv instance (create on the main thread, [release] exactly once). Options follow mpv-android's defaults for a SurfaceView
 * (`vo=gpu`, `gpu-context=android`, `hwdec=mediacodec-copy` with software fallback) plus a long network timeout and a large demuxer
 * cache for torrent loopback streams. ASS subtitles are rendered by libass with their own styles (`sub-ass-override=no`).
 * TLS: mpv's default (`tls-verify=no`, there is no CA bundle in the AAR), as in mpv-android without its bundled cacert.
 * State updates come from mpv's event thread; [state] is safe to collect anywhere.
 */
class MpvEngine(context: Context) {
    private val appContext = context.applicationContext
    private val _state = MutableStateFlow(MpvState(buffering = false))
    val state: StateFlow<MpvState> get() = _state.asStateFlow()

    private val mpv: MPVLib = checkNotNull(MPVLib.create(appContext)) { "libmpv could not create an instance" }

    @Volatile
    private var released = false

    @Volatile
    private var fileLoaded = false

    @Volatile
    private var lastError: String? = null

    /** External subtitle URLs of the current load, added with `sub-add` once the file is loaded. */
    @Volatile
    private var pendingSubtitles: List<String> = emptyList()

    private val observer = object : MPVLib.EventObserver {
        override fun eventProperty(property: String) {
            if (property == "track-list") refreshTracks()
        }

        override fun eventProperty(property: String, value: Long) {
            if (property == "height") _state.update { it.copy(videoHeight = value.toInt()) }
        }

        override fun eventProperty(property: String, value: Double) {
            when (property) {
                "time-pos" -> _state.update { it.copy(positionSecs = value.coerceAtLeast(0.0)) }
                "duration" -> _state.update { it.copy(durationSecs = value.coerceAtLeast(0.0)) }
                "speed" -> _state.update { it.copy(speed = value) }
                "demuxer-cache-time" -> _state.update { it.copy(cacheSecs = value.coerceAtLeast(0.0)) }
            }
        }

        override fun eventProperty(property: String, value: Boolean) {
            when (property) {
                "pause" -> _state.update { it.copy(playing = !value && fileLoaded && !it.ended) }
                "paused-for-cache" -> _state.update { it.copy(buffering = value || !it.started) }
                "eof-reached" -> if (value && fileLoaded) _state.update { it.copy(ended = true, playing = false, buffering = false) }
                "core-idle" -> _state.update { s ->
                    val paused = mpv.getPropertyBoolean("pause") == true
                    s.copy(playing = !value && !paused && fileLoaded && !s.ended)
                }
            }
        }

        override fun eventProperty(property: String, value: String) {
            if (property == "track-list") refreshTracks()
        }

        override fun event(eventId: Int) {
            when (eventId) {
                MPVLib.MpvEvent.MPV_EVENT_START_FILE -> {
                    fileLoaded = false
                    _state.update { it.copy(buffering = true, ended = false, error = null, started = false) }
                }
                MPVLib.MpvEvent.MPV_EVENT_FILE_LOADED -> {
                    fileLoaded = true
                    for (url in pendingSubtitles) mpv.command(arrayOf("sub-add", url, "auto"))
                    refreshTracks()
                }
                MPVLib.MpvEvent.MPV_EVENT_PLAYBACK_RESTART -> _state.update {
                    it.copy(started = true, buffering = false, playing = mpv.getPropertyBoolean("pause") != true)
                }
                MPVLib.MpvEvent.MPV_EVENT_END_FILE -> {
                    // keep-open=yes: a normal end does not unload the file, so END_FILE before FILE_LOADED is a load failure.
                    if (!fileLoaded && !released) {
                        _state.update { it.copy(error = lastError ?: "loading failed", buffering = false, playing = false) }
                    }
                }
            }
        }
    }

    private val logObserver = object : MPVLib.LogObserver {
        override fun logMessage(prefix: String, level: Int, text: String) {
            if (level <= MPVLib.MpvLogLevel.MPV_LOG_LEVEL_ERROR) lastError = "$prefix: ${text.trim()}"
        }
    }

    init {
        mpv.setOptionString("vo", "gpu")
        mpv.setOptionString("gpu-context", "android")
        mpv.setOptionString("opengl-es", "yes")
        mpv.setOptionString("hwdec", "mediacodec-copy,mediacodec,no")
        mpv.setOptionString("hwdec-codecs", "h264,hevc,mpeg4,mpeg2video,vp8,vp9,av1")
        mpv.setOptionString("ao", "audiotrack,opensles")
        mpv.setOptionString("input-default-bindings", "no")
        mpv.setOptionString("idle", "yes")
        mpv.setOptionString("keep-open", "yes")
        mpv.setOptionString("force-window", "no")
        mpv.setOptionString("save-position-on-quit", "no")
        mpv.setOptionString("cache", "yes")
        mpv.setOptionString("demuxer-max-bytes", "${DEMUXER_MAX_MB}MiB")
        mpv.setOptionString("demuxer-max-back-bytes", "${DEMUXER_BACK_MB}MiB")
        mpv.setOptionString("demuxer-readahead-secs", "$READAHEAD_SECS")
        mpv.setOptionString("network-timeout", "$NETWORK_TIMEOUT_S")
        mpv.setOptionString("sub-ass-override", "no")
        mpv.init()
        mpv.addObserver(observer)
        mpv.addLogObserver(logObserver)
        mpv.observeProperty("time-pos", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        mpv.observeProperty("duration", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        mpv.observeProperty("speed", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        mpv.observeProperty("demuxer-cache-time", MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
        mpv.observeProperty("pause", MPVLib.MpvFormat.MPV_FORMAT_FLAG)
        mpv.observeProperty("paused-for-cache", MPVLib.MpvFormat.MPV_FORMAT_FLAG)
        mpv.observeProperty("eof-reached", MPVLib.MpvFormat.MPV_FORMAT_FLAG)
        mpv.observeProperty("core-idle", MPVLib.MpvFormat.MPV_FORMAT_FLAG)
        mpv.observeProperty("height", MPVLib.MpvFormat.MPV_FORMAT_INT64)
        mpv.observeProperty("track-list", MPVLib.MpvFormat.MPV_FORMAT_NONE)
    }

    /** Load [url] with HTTP [headers] (`http-header-fields`), start at [startSecs]. */
    fun load(url: String, headers: Map<String, String>, startSecs: Double = 0.0) = load(url, headers, startSecs, emptyList())

    /** [load] plus external subtitle URLs (`sub-add` after the file is loaded). */
    fun load(url: String, headers: Map<String, String>, startSecs: Double, subtitleUrls: List<String>) {
        if (released) return
        fileLoaded = false
        lastError = null
        pendingSubtitles = subtitleUrls
        _state.update { MpvState(loadId = it.loadId + 1, buffering = true, speed = it.speed) }
        val ua = headers.entries.firstOrNull { it.key.equals("User-Agent", ignoreCase = true) }?.value
        val referer = headers.entries.firstOrNull { it.key.equals("Referer", ignoreCase = true) }?.value
        val rest = headers.filterKeys { !it.equals("User-Agent", ignoreCase = true) && !it.equals("Referer", ignoreCase = true) }
        mpv.setPropertyString("user-agent", ua ?: DEFAULT_USER_AGENT)
        mpv.setPropertyString("referrer", referer ?: "")
        // List option: entries separated by ',', so commas inside values are escaped mpv-style.
        mpv.setPropertyString("http-header-fields", rest.entries.joinToString(",") { (k, v) -> "$k: ${v.replace(",", "\\,")}" })
        mpv.setPropertyString("start", if (startSecs > 0) String.format(Locale.ROOT, "%.3f", startSecs) else "none")
        val start = {
            mpv.command(arrayOf("loadfile", url, "replace"))
            mpv.setPropertyBoolean("pause", false)
        }
        // vo=gpu without a surface fails for good ("Failed initializing any suitable GPU context") and mpv goes on audio-only, so the
        // file is loaded once the SurfaceView is attached (mpv-android loads in surfaceCreated too). Seen when the candidates come from
        // the cache and the load beats surfaceCreated.
        val now = synchronized(surfaceLock) {
            if (surfaceAttached) true else { pendingStart = start; false }
        }
        if (now) start()
    }

    fun play() {
        if (released) return
        if (_state.value.ended) seekTo(0.0)
        mpv.setPropertyBoolean("pause", false)
    }

    fun pause() {
        if (released) return
        mpv.setPropertyBoolean("pause", true)
    }

    fun seekTo(secs: Double) {
        if (released) return
        _state.update { it.copy(ended = false) }
        mpv.command(arrayOf("seek", String.format(Locale.ROOT, "%.3f", secs.coerceAtLeast(0.0)), "absolute"))
    }

    fun setSpeed(speed: Double) {
        if (!released) mpv.setPropertyDouble("speed", speed)
    }

    /** mpv `sid`; null = subtitles off. */
    fun selectSubtitle(id: Int?) {
        if (released) return
        mpv.setPropertyString("sid", id?.toString() ?: "no")
        refreshTracks()
    }

    /** mpv `aid`. */
    fun selectAudio(id: Int) {
        if (released) return
        mpv.setPropertyString("aid", id.toString())
        refreshTracks()
    }

    fun setPreferredLanguages(subtitle: String?, audio: String?) {
        if (released) return
        subtitle?.takeIf { it.isNotBlank() }?.let { mpv.setPropertyString("slang", it) }
        audio?.takeIf { it.isNotBlank() }?.let { mpv.setPropertyString("alang", it) }
    }

    /**
     * Subtitle look for text subtitles (SRT / VTT; ASS keeps its own styles): `sub-scale`, `sub-bold`, `sub-border-size`, a translucent
     * box (`sub-border-style=background-box` + `sub-back-color`) when [backgroundOpacity] > 0. The app font is not passed to libass
     * (it would need a fonts directory on disk): mpv uses its default sans font.
     */
    fun setSubtitleStyle(scale: Float, backgroundOpacity: Float, bold: Boolean, outline: Boolean) {
        if (released) return
        mpv.setPropertyDouble("sub-scale", scale.coerceIn(0.5f, 2f).toDouble())
        mpv.setPropertyBoolean("sub-bold", bold)
        mpv.setPropertyDouble("sub-border-size", if (outline) 2.5 else 0.0)
        val alpha = backgroundOpacity.coerceIn(0f, 1f)
        if (alpha > 0f) {
            mpv.setPropertyString("sub-border-style", "background-box")
            mpv.setPropertyString("sub-back-color", String.format(Locale.ROOT, "0.0/0.0/0.0/%.2f", alpha))
        } else {
            mpv.setPropertyString("sub-border-style", "outline-and-shadow")
        }
    }

    /**
     * Applies a shader preset (Settings › Reproductor › Preajuste) when its GLSL files exist under `filesDir/shaders/<preset>/`. Anime4K /
     * FSR are not bundled in this wave (12 §7.13): without files this clears the chain and returns false (the preset is ignored).
     */
    fun applyPreset(preset: String): Boolean {
        if (released) return false
        val dir = File(appContext.filesDir, "shaders/$preset")
        val files = if (preset != "off" && dir.isDirectory) {
            dir.listFiles { f -> f.extension == "glsl" }?.sortedBy { it.name }.orEmpty()
        } else {
            emptyList()
        }
        mpv.setPropertyString("glsl-shaders", files.joinToString(PATH_LIST_SEPARATOR) { it.absolutePath })
        return files.isNotEmpty()
    }

    fun currentPositionSecs(): Double = if (released) 0.0 else mpv.getPropertyDouble("time-pos") ?: _state.value.positionSecs

    private val surfaceLock = Any()
    private var surfaceAttached = false
    private var pendingStart: (() -> Unit)? = null

    internal fun attachSurface(surface: Surface) {
        if (released) return
        mpv.attachSurface(surface)
        mpv.setOptionString("force-window", "yes")
        mpv.setPropertyString("vo", "gpu")
        val start = synchronized(surfaceLock) {
            surfaceAttached = true
            pendingStart.also { pendingStart = null }
        }
        start?.invoke()
    }

    internal fun resizeSurface(width: Int, height: Int) {
        if (!released) mpv.setPropertyString("android-surface-size", "${width}x$height")
    }

    internal fun detachSurface() {
        synchronized(surfaceLock) { surfaceAttached = false }
        if (released) return
        mpv.setPropertyString("vo", "null")
        mpv.setOptionString("force-window", "no")
        mpv.detachSurface()
    }

    fun release() {
        if (released) return
        released = true
        synchronized(surfaceLock) { pendingStart = null }
        mpv.removeObserver(observer)
        mpv.removeLogObserver(logObserver)
        mpv.destroy()
    }

    private fun refreshTracks() {
        if (released) return
        val count = mpv.getPropertyInt("track-list/count") ?: return
        val subs = mutableListOf<MpvTrack>()
        val audio = mutableListOf<MpvTrack>()
        for (i in 0 until count) {
            val type = mpv.getPropertyString("track-list/$i/type") ?: continue
            val id = mpv.getPropertyInt("track-list/$i/id") ?: continue
            val track = MpvTrack(
                id = id,
                title = mpv.getPropertyString("track-list/$i/title"),
                lang = mpv.getPropertyString("track-list/$i/lang"),
                codec = mpv.getPropertyString("track-list/$i/codec"),
                selected = mpv.getPropertyBoolean("track-list/$i/selected") == true,
            )
            when (type) {
                "sub" -> subs += track
                "audio" -> audio += track
            }
        }
        _state.update { it.copy(subtitles = subs, audio = audio) }
    }

    companion object {
        const val DEMUXER_MAX_MB = 150
        const val DEMUXER_BACK_MB = 50
        const val READAHEAD_SECS = 60
        const val NETWORK_TIMEOUT_S = 90
        private const val DEFAULT_USER_AGENT = "TatakaiNext/0.1"

        /** mpv path-list separator on Android (POSIX). */
        private const val PATH_LIST_SEPARATOR = ":"
    }
}
