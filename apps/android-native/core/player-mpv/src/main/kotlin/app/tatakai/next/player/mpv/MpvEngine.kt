// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.content.Context
import android.view.Surface
import app.tatakai.next.player.LanguageCodes
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
data class MpvTrack(
    val id: Int,
    val title: String?,
    val lang: String?,
    val codec: String?,
    val selected: Boolean,
    /** 0.4.0 (sub-translation): `track-list/N/external` - added with `sub-add`, not stored in the media file. */
    val external: Boolean = false,
)

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
    /** Timestamp (seconds) where the buffered data ends (`demuxer-cache-time`; 0.4.1: an absolute time, not a duration). */
    val cacheSecs: Double = 0.0,
    val subtitles: List<MpvTrack> = emptyList(),
    val audio: List<MpvTrack> = emptyList(),
    /** Video height of the current file (`height`), 0 = unknown. */
    val videoHeight: Int = 0,
    /** 0.4.0 (worker A): video width of the current file (`width`), 0 = unknown. */
    val videoWidth: Int = 0,
    /** 0.4.0 (worker A): size of the attached surface in pixels (the view the video is fitted into), 0 = none. */
    val surfaceWidth: Int = 0,
    val surfaceHeight: Int = 0,
    // --- parity wave A (worker p) ---
    /** Primary subtitle line on screen right now (`sub-text`), for the screen-drawn translation. */
    val subText: String? = null,
    /** `secondary-sid`, null = off. */
    val secondarySid: Int? = null,
    /** `loop-file=inf`. */
    val loop: Boolean = false,
    /** `sub-delay` in ms. */
    val subDelayMs: Long = 0,
)

/** One external subtitle for `sub-add`: [title] / [lang] as mpv lists it; [select] = the active track. */
data class MpvSubtitle(val url: String, val title: String? = null, val lang: String? = null, val select: Boolean = false)

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

    /** External subtitles of the current load, added with `sub-add` once the file is loaded. */
    @Volatile
    private var pendingSubtitles: List<MpvSubtitle> = emptyList()

    /** 0.4.0: the current load asked for its subtitles off ([MpvSubtitle.select] for none of them, see [loadPrepared]). */
    @Volatile
    private var pendingSubtitlesOff = false

    /** 0.4.0: [addSubtitles] before FILE_LOADED appends to the pending ones; this keeps the two threads from losing any. */
    private val subtitleLock = Any()

    private val observer = object : MPVLib.EventObserver {
        override fun eventProperty(property: String) {
            if (property == "track-list") refreshTracks()
        }

        override fun eventProperty(property: String, value: Long) {
            when (property) {
                "height" -> _state.update { it.copy(videoHeight = value.toInt()) }
                "width" -> _state.update { it.copy(videoWidth = value.toInt()) }
            }
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
            when (property) {
                "track-list" -> refreshTracks()
                "sub-text" -> _state.update { it.copy(subText = value.takeIf { v -> v.isNotBlank() }) }
            }
        }

        override fun event(eventId: Int) {
            when (eventId) {
                MPVLib.MpvEvent.MPV_EVENT_START_FILE -> {
                    fileLoaded = false
                    _state.update { it.copy(buffering = true, ended = false, error = null, started = false) }
                }
                MPVLib.MpvEvent.MPV_EVENT_FILE_LOADED -> {
                    fileLoaded = true
                    val (subs, off) = synchronized(subtitleLock) { pendingSubtitles to pendingSubtitlesOff }
                    addPending(subs, off)
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
            logListener?.invoke(level, prefix, text)
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
        // parity wave A (player.subs.custom-fonts): fonts imported by the user (`filesDir/fonts`) are visible to libass, also for the
        // fonts ASS styles ask for; embedded MKV fonts keep working (`embeddedfonts` default yes).
        val fontsDir = File(appContext.filesDir, "fonts").apply { mkdirs() }
        // 0.4.1: static Outfit / Space Grotesk faces for the "Outfit" and "Space Grotesk" subtitle fonts (libass draws a variable
        // font at its default instance, Thin / Light). Installed before init: libass reads the folder when a track starts.
        installBundledSubtitleFonts(fontsDir)
        mpv.setOptionString("sub-fonts-dir", fontsDir.absolutePath)
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
        mpv.observeProperty("width", MPVLib.MpvFormat.MPV_FORMAT_INT64)
        mpv.observeProperty("track-list", MPVLib.MpvFormat.MPV_FORMAT_NONE)
        mpv.observeProperty("sub-text", MPVLib.MpvFormat.MPV_FORMAT_STRING)
    }

    /** Load [url] with HTTP [headers] (`http-header-fields`), start at [startSecs]. */
    fun load(url: String, headers: Map<String, String>, startSecs: Double = 0.0) = load(url, headers, startSecs, emptyList())

    /** [load] plus external subtitle URLs (`sub-add` after the file is loaded). */
    fun load(url: String, headers: Map<String, String>, startSecs: Double, subtitleUrls: List<String>) {
        if (released) return
        beginLoad()
        loadPrepared(url, headers, startSecs, subtitleUrls.map { MpvSubtitle(it) }, lavfOptions = "")
    }

    /**
     * BUG-059: starts a new load (new `loadId`, buffering) before its URL is known, so the session can prepare the stream (a filtered
     * HLS master) while the player already shows the new load. Returns its `loadId`; [loadPrepared] then opens the URL.
     */
    fun beginLoad(): Int {
        if (released) return _state.value.loadId
        fileLoaded = false
        lastError = null
        subtitleDecided = false
        _state.update {
            MpvState(
                loadId = it.loadId + 1, buffering = true, speed = it.speed, loop = it.loop, subDelayMs = it.subDelayMs,
                surfaceWidth = it.surfaceWidth, surfaceHeight = it.surfaceHeight,
            )
        }
        return _state.value.loadId
    }

    /** Opens [url] for the load [beginLoad] started (see [load]). */
    fun loadPrepared(
        url: String,
        headers: Map<String, String>,
        startSecs: Double,
        subtitles: List<MpvSubtitle>,
        lavfOptions: String = "",
        subtitlesOff: Boolean = false,
    ) {
        if (released) return
        synchronized(subtitleLock) {
            pendingSubtitles = subtitles
            pendingSubtitlesOff = subtitlesOff
        }
        // BUG-080: set on every load, so the widened protocol whitelist of a filtered local master does not leak into the next one.
        mpv.setPropertyString("demuxer-lavf-o", lavfOptions)
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

    /**
     * BUG-059: give up on the current load when it produced no frame yet: stop mpv (FFmpeg may otherwise keep waiting on an HLS
     * rendition that has no segments) and report [reason] as the load's error. No-op once playback started.
     */
    fun failStart(reason: String) {
        if (released || _state.value.started) return
        lastError = reason
        mpv.command(arrayOf("stop"))
        _state.update { it.copy(error = reason, buffering = false, playing = false) }
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
        subtitleDecided = true
        mpv.setPropertyString("sid", id?.toString() ?: "no")
        refreshTracks()
    }

    /** mpv `aid`. */
    fun selectAudio(id: Int) {
        if (released) return
        mpv.setPropertyString("aid", id.toString())
        refreshTracks()
    }

    /**
     * Bug player.subs.preferred-lang: `slang` / `alang` get every variant of the language (`es-419,es-ES,es-MX,es,spa,esl`), and the first
     * track list of a load selects a matching subtitle when mpv chose none ([refreshTracks]).
     */
    fun setPreferredLanguages(subtitle: String?, audio: String?) {
        if (released) return
        preferredSub = LanguageCodes.normalize(subtitle)
        LanguageCodes.engineVariants(subtitle).takeIf { it.isNotEmpty() }?.let { mpv.setPropertyString("slang", it.joinToString(",")) }
        LanguageCodes.engineVariants(audio).takeIf { it.isNotEmpty() }?.let { mpv.setPropertyString("alang", it.joinToString(",")) }
    }

    @Volatile
    private var preferredSub: String? = null

    @Volatile
    private var subtitleDecided = false

    /** `sub-delay` (seconds), ±60 s. */
    fun setSubtitleDelay(ms: Long) {
        if (released) return
        val clamped = app.tatakai.next.player.SubtitleDelay.clamp(ms)
        mpv.setPropertyDouble("sub-delay", clamped / 1000.0)
        _state.update { it.copy(subDelayMs = clamped) }
    }

    /**
     * 0.4.0: `sub-add` of [subs] to the file playing now (the PC's subtitle tracks once it extracted them); [off] = none selected.
     * Blocks while mpv reads them: call it off the main thread.
     */
    fun addSubtitles(subs: List<MpvSubtitle>, off: Boolean) {
        if (released) return
        val now = synchronized(subtitleLock) {
            if (fileLoaded) return@synchronized true
            // still opening: they go with the file's own (FILE_LOADED)
            pendingSubtitles = pendingSubtitles + subs
            pendingSubtitlesOff = pendingSubtitlesOff || off
            false
        }
        if (!now) return
        addPending(subs, off)
        refreshTracks()
    }

    /** `sub-add <url> select|auto [<title> [<lang>]]` of each; a load that names its track (or none) is not auto-picked again. */
    private fun addPending(subs: List<MpvSubtitle>, off: Boolean) {
        if (subs.isEmpty()) return
        if (off || subs.any { it.select }) subtitleDecided = true
        for (s in subs) {
            val cmd = mutableListOf("sub-add", s.url, if (s.select) "select" else "auto")
            if (s.title != null || s.lang != null) cmd += s.title ?: ""
            if (s.lang != null) cmd += s.lang
            runCatching { mpv.command(cmd.toTypedArray()) }
        }
        if (off) mpv.setPropertyString("sid", "no")
    }

    /** `sub-add <path> select <title>`: a loaded SRT / ASS / VTT becomes the active track. */
    fun addSubtitleFile(path: String, title: String) {
        if (released) return
        subtitleDecided = true
        mpv.command(arrayOf("sub-add", path, "select", title))
        refreshTracks()
    }

    /** `secondary-sid` (drawn at the top by mpv); null = off. */
    fun selectSecondarySubtitle(id: Int?) {
        if (released) return
        mpv.setPropertyString("secondary-sid", id?.toString() ?: "no")
        _state.update { it.copy(secondarySid = id) }
    }

    /** `sub-visibility` (the screen draws translated lines itself while this is off). */
    fun setSubtitlesVisible(visible: Boolean) {
        if (!released) mpv.setPropertyBoolean("sub-visibility", visible)
    }

    fun setLoop(loop: Boolean) {
        if (released) return
        mpv.setPropertyString("loop-file", if (loop) "inf" else "no")
        _state.update { it.copy(loop = loop, ended = if (loop) false else it.ended) }
    }

    /** Stable volume: ffmpeg `dynaudnorm` through mpv's `af` (classic `usePlayerEnhancements`). */
    fun setStableVolume(enabled: Boolean) {
        if (!released) mpv.setPropertyString("af", if (enabled) STABLE_VOLUME_FILTER else "")
    }

    /** `screenshot-to-file <path> video` (the frame without subtitles or OSD). True when the PNG exists afterwards. */
    fun screenshotToFile(path: String): Boolean {
        if (released) return false
        val f = File(path)
        f.delete()
        f.parentFile?.mkdirs()
        mpv.command(arrayOf("screenshot-to-file", path, "video"))
        return f.isFile && f.length() > 0
    }

    // 0.4.1: the subtitle look is written by MpvPlayerSession through [setProperty] (MpvSubtitleStyle.kt: every sub-* option, only
    // the ones that changed); 0.4.0's setSubtitleStyle / setSubtitleExtras are gone.

    /**
     * Copies `assets/subfonts/` (named `tatakai_*`, hidden from the imported-fonts list) into [dir] when missing. Assets may be stored
     * compressed (no `openFd` size check): a changed font ships under a new file name instead.
     */
    private fun installBundledSubtitleFonts(dir: File) {
        val assets = appContext.assets
        runCatching { assets.list(BUNDLED_FONTS_ASSETS).orEmpty() }.getOrDefault(emptyArray()).forEach { name ->
            val target = File(dir, name)
            if (target.isFile && target.length() > 0) return@forEach
            runCatching {
                val tmp = File(dir, "$name.part")
                assets.open("$BUNDLED_FONTS_ASSETS/$name").use { src -> tmp.outputStream().use { src.copyTo(it) } }
                if (!tmp.renameTo(target)) tmp.delete()
            }
        }
    }

    /**
     * 0.4.0 (worker A): mpv log lines (level, prefix, text) for the quality engine's shader-error detection. Called on mpv's event
     * thread: keep it cheap and hand any work to another thread.
     */
    @Volatile
    var logListener: ((Int, String, String) -> Unit)? = null

    /**
     * 0.4.0 (worker A): raw property access for `MpvQualityController` (glsl-shaders, deband, image, aspect, hwdec, interpolation and
     * the telemetry reads). The shader chain and every quality option are owned by the controller; 0.3.x's `applyPreset` is gone.
     */
    internal fun setProperty(name: String, value: String) {
        if (!released) mpv.setPropertyString(name, value)
    }

    internal fun getProperty(name: String): String? = if (released) null else mpv.getPropertyString(name)

    /** BUG-043: frames mpv dropped so far in this file (`frame-drop-count` + `decoder-frame-drop-count`); null when unknown. */
    fun droppedFrames(): Long? {
        if (released) return null
        val vo = mpv.getPropertyInt("frame-drop-count")
        val dec = mpv.getPropertyInt("decoder-frame-drop-count")
        if (vo == null && dec == null) return null
        return (vo ?: 0).toLong() + (dec ?: 0).toLong()
    }

    /** The video's frame rate (`container-fps`, else `estimated-vf-fps`); null when unknown. */
    fun videoFps(): Double? {
        if (released) return null
        return (mpv.getPropertyDouble("container-fps") ?: mpv.getPropertyDouble("estimated-vf-fps"))?.takeIf { it > 0 }
    }

    fun currentPositionSecs(): Double = if (released) 0.0 else mpv.getPropertyDouble("time-pos") ?: _state.value.positionSecs

    /**
     * 0.4.0 (worker B, phone FG): MpvFrameGenBridge drives this same libmpv instance (video-params, audio-delay, vid, its own
     * observer). The output surface still only moves through [attachSurface] / [resizeSurface] / [detachSurface].
     */
    internal val frameGenLib: MPVLib get() = mpv

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
        if (released) return
        mpv.setPropertyString("android-surface-size", "${width}x$height")
        _state.update { it.copy(surfaceWidth = width, surfaceHeight = height) }
        // BUG-093: a paused vo=gpu keeps the last frame laid out for the old size (stretched, black band) until playback resumes; an
        // exact seek to the same position redraws it for the new size.
        val st = _state.value
        if (needsRedrawAfterResize(paused = mpv.getPropertyBoolean("pause") == true, started = st.started, ended = st.ended)) {
            mpv.command(arrayOf("seek", "0", "relative+exact"))
        }
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
                external = mpv.getPropertyBoolean("track-list/$i/external") == true,
            )
            when (type) {
                "sub" -> subs += track
                "audio" -> audio += track
            }
        }
        _state.update { it.copy(subtitles = subs, audio = audio) }
        // First track list of a load with subtitles and none selected: pick the preferred language (bug player.subs.preferred-lang).
        if (fileLoaded && !subtitleDecided && subs.isNotEmpty()) {
            subtitleDecided = true
            if (subs.none { it.selected }) {
                LanguageCodes.firstMatch(subs, preferredSub, { it.lang }, { it.title })?.let { i -> selectSubtitle(subs[i].id) }
            }
        }
    }

    companion object {
        const val DEMUXER_MAX_MB = 150
        const val DEMUXER_BACK_MB = 50
        const val READAHEAD_SECS = 60
        const val NETWORK_TIMEOUT_S = 90
        private const val DEFAULT_USER_AGENT = "TatakaiNext/0.1"
        private const val BUNDLED_FONTS_ASSETS = "subfonts"
        const val STABLE_VOLUME_FILTER ="lavfi=[dynaudnorm=f=250:g=15:p=0.9]"
    }
}

/** BUG-093: whether a surface resize must redraw the frame by hand (paused on a frame; while playing the next frame does it). */
internal fun needsRedrawAfterResize(paused: Boolean, started: Boolean, ended: Boolean): Boolean = paused && started && !ended
