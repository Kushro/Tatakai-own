// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import android.os.Handler
import android.os.Looper
import android.view.Surface
import android.view.SurfaceView
import android.view.View
import app.tatakai.next.framegen.FgController
import app.tatakai.next.framegen.FgMpvPort
import app.tatakai.next.framegen.SurfaceFrameRate
import dev.jdtech.mpv.MPVLib
import java.util.WeakHashMap

/**
 * 0.4.0 (worker B): the seam between one [MpvEngine] and the phone frame generation ([FgController], :core:framegen).
 *
 * - As [FgMpvPort] it lets the controller move mpv's video output between the player SurfaceView and the FG input surface
 *   (an AImageReader `Surface`): [connect] / [disconnect] reuse the engine's own `attachSurface` / `detachSurface` (mpv-android
 *   order, the engine's `vo`, its "surface attached" bookkeeping for loads that wait for a surface) plus `resizeSurface` for
 *   `android-surface-size`. libmpv 1.0.0 renders into any `Surface` (`MPVLib.attachSurface` sets `wid` to it and vo_gpu's EGL
 *   context draws into that window), which is what Enigmata's mpvlib does too.
 * - [MpvPlayerSurface] forwards the SurfaceView callbacks ([onSurfaceCreated] / [onSurfaceChanged] / [onSurfaceDestroyed]) so
 *   that, while FG presents on the SurfaceView, mpv is not re-attached to it nor resized to it.
 * - It observes the mpv properties / events the controller needs (pause, cache, seeking, container-fps, video-params,
 *   FILE_LOADED, SEEK, PLAYBACK_RESTART) on a second `EventObserver` and posts them to the main thread.
 *
 * Created by [MpvPlayerSession] when the FG controller is bound in Koin; [release] runs before the engine is destroyed.
 * Main thread, except the mpv observer callbacks (posted).
 */
internal class MpvFrameGenBridge(
  private val engine: MpvEngine,
  private val controller: FgController,
) : FgMpvPort {
  private val lib: MPVLib = engine.frameGenLib
  private val main = Handler(Looper.getMainLooper())
  private var view: SurfaceView? = null

  @Volatile
  private var released = false

  private val observer = object : MPVLib.EventObserver {
    override fun eventProperty(property: String) = Unit

    override fun eventProperty(property: String, value: Long) {
      if (property.startsWith(VIDEO_PARAMS)) post { controller.onVideoGeometryChanged() }
    }

    override fun eventProperty(property: String, value: Double) {
      if (property == CONTAINER_FPS) post { controller.onContainerFps(value) }
    }

    override fun eventProperty(property: String, value: Boolean) {
      when (property) {
        "pause" -> post { controller.onPause(value) }
        "paused-for-cache" -> post { controller.onPausedForCache(value) }
        "seeking" -> post { controller.onSeeking(value) }
      }
    }

    override fun eventProperty(property: String, value: String) = Unit

    override fun event(eventId: Int) {
      when (eventId) {
        MPVLib.MpvEvent.MPV_EVENT_FILE_LOADED -> post {
          controller.onFileLoaded()
          // The fps / geometry of the new file may already be known (no property change follows for equal values).
          lib.getPropertyDouble(CONTAINER_FPS)?.let { controller.onContainerFps(it) }
          controller.onVideoGeometryChanged()
        }
        MPVLib.MpvEvent.MPV_EVENT_SEEK -> post { controller.onSeek() }
        MPVLib.MpvEvent.MPV_EVENT_PLAYBACK_RESTART -> post { controller.onSeekFinished() }
        MPVLib.MpvEvent.MPV_EVENT_VIDEO_RECONFIG -> post { controller.onVideoGeometryChanged() }
      }
    }
  }

  init {
    lib.addObserver(observer)
    // Extra observers on the same instance: the engine's observer ignores names it does not handle.
    lib.observeProperty(CONTAINER_FPS, MPVLib.MpvFormat.MPV_FORMAT_DOUBLE)
    lib.observeProperty("seeking", MPVLib.MpvFormat.MPV_FORMAT_FLAG)
    lib.observeProperty("$VIDEO_PARAMS/dw", MPVLib.MpvFormat.MPV_FORMAT_INT64)
    lib.observeProperty("$VIDEO_PARAMS/dh", MPVLib.MpvFormat.MPV_FORMAT_INT64)
    lib.observeProperty("$VIDEO_PARAMS/rotate", MPVLib.MpvFormat.MPV_FORMAT_INT64)
    synchronized(bridges) { bridges[engine] = this }
    controller.bind(this)
  }

  /** Before `engine.release()`: FG off without reconnecting mpv, then unbound. Idempotent. */
  fun release() {
    if (released) return
    if (controller.isBound(this)) controller.onNativeDestroying()
    controller.unbind(this)
    released = true
    main.removeCallbacksAndMessages(null)
    lib.removeObserver(observer)
    synchronized(bridges) { if (bridges[engine] === this) bridges.remove(engine) }
    view = null
  }

  private inline fun post(crossinline block: () -> Unit) {
    main.post { if (!released && controller.isBound(this)) block() }
  }

  // --- SurfaceView callbacks (MpvPlayerSurface) -----------------------------------------------------------------------------

  /** After the engine attached mpv to the new SurfaceView Surface. */
  fun onSurfaceCreated(surfaceView: SurfaceView) {
    view = surfaceView
    if (!released) controller.onOutputSurfaceCreated()
  }

  /** True when FG handled the change (the engine must not resize: mpv renders into the FG input). */
  fun onSurfaceChanged(width: Int, height: Int): Boolean = !released && controller.onOutputSurfaceChanged(width, height)

  /** True when FG handled the destroy (mpv is not on the SurfaceView: the engine must not detach again). */
  fun onSurfaceDestroyed(): Boolean = !released && controller.onOutputSurfaceDestroyed()

  // --- FgMpvPort --------------------------------------------------------------------------------------------------------------

  override fun connect(surface: Surface, width: Int, height: Int) {
    engine.attachSurface(surface)
    if (width > 0 && height > 0) engine.resizeSurface(width, height)
  }

  override fun disconnect() = engine.detachSurface()

  override fun setPropertyString(name: String, value: String) {
    lib.setPropertyString(name, value)
  }

  override fun getPropertyString(name: String): String? = lib.getPropertyString(name)

  override fun getPropertyInt(name: String): Int? = lib.getPropertyInt(name)

  override fun getPropertyDouble(name: String): Double? = lib.getPropertyDouble(name)

  override fun getAudioDelaySec(): Double? = lib.getPropertyDouble(AUDIO_DELAY)

  override fun setAudioDelaySec(sec: Double) {
    lib.setPropertyDouble(AUDIO_DELAY, sec)
  }

  override fun currentPath(): String? = lib.getPropertyString("path")

  override fun outputSurface(): Surface? = view?.holder?.surface?.takeIf { it.isValid }

  override fun outputSize(): Pair<Int, Int>? {
    val frame = view?.holder?.surfaceFrame ?: return null
    return if (frame.width() > 0 && frame.height() > 0) frame.width() to frame.height() else null
  }

  override fun setOutputVisible(visible: Boolean) {
    view?.visibility = if (visible) View.VISIBLE else View.INVISIBLE
  }

  override fun applyFrameRate(fps: Double, alwaysChange: Boolean) = SurfaceFrameRate.apply(view?.holder?.surface, fps, alwaysChange)

  override fun clearFrameRate() = SurfaceFrameRate.clear(view?.holder?.surface)

  /** An upscaler chain is set (`glsl-shaders` not empty), whoever set it (preset, governor, in-player quality menu). */
  override fun upscalerActive(): Boolean = !lib.getPropertyString("glsl-shaders").isNullOrBlank()

  companion object {
    private const val AUDIO_DELAY = "audio-delay"
    private const val CONTAINER_FPS = "container-fps"
    private const val VIDEO_PARAMS = "video-params"

    /** The bridge of each live engine, so [MpvPlayerSurface] (which only gets the engine) can route its callbacks. */
    private val bridges = WeakHashMap<MpvEngine, MpvFrameGenBridge>()

    fun of(engine: MpvEngine): MpvFrameGenBridge? = synchronized(bridges) { bridges[engine] }
  }
}
