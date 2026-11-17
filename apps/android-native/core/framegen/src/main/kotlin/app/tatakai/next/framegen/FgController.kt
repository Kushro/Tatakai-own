/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import android.net.Uri
import android.util.Log
import android.view.Surface
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.util.Locale
import kotlin.math.max
import kotlin.math.min

/**
 * R13 frame generation: owns the FG lifecycle for the player (Enigmata facts 2, 4, 5, 8, 9, 13-17, 19, 21, 25). Implements
 * [FrameGenController] for the player UI, Tatakai Link and Settings. Ported from Enigmata `fg/FgController.kt`; the Tatakai
 * differences are marked «Tatakai».
 *
 * Tatakai wiring: one app-wide instance (Koin). An MPV session binds its [FgMpvPort] (`MpvFrameGenBridge`, :core:player-mpv)
 * with [bind] and forwards mpv events and SurfaceView callbacks to the `on*` methods below; [unbind] with the same port when the
 * session is released (after [onNativeDestroying], before `MPVLib.destroy`).
 *
 * Output routing (the seam of :core:player-mpv): normally mpv renders into the player SurfaceView. While FG is active mpv
 * renders into the FG input (an AImageReader `Surface`, [FgNative.createInputSurface]); the native engine imports each frame
 * (AHardwareBuffer -> Vulkan), generates N-1 frames between real ones with LSFG and presents them on the SurfaceView's Surface
 * with its own Vulkan swapchain.
 *
 * Sequences on the main thread (the step numbers are Enigmata's design and are repeated in the code):
 * - Turn on ([turnOn]):
 *   1. Eligibility ([FgPolicy.eligibility]), then N (preference or [FgPolicy.autoMultiplier]) and the tuning
 *      ([FgPolicy.resolveProfile]).
 *   2. `inS = createInputSurface(inputSize)`.
 *   3. mpv: [FgMpvPort.disconnect].
 *   4. `start(SurfaceView surface, cfg)`. On failure: `stop()`, reconnect mpv to the SurfaceView, show the reason and return.
 *   5. [FgActiveFlag] = true (right before mpv opens its VO on the input), [FgMpvPort.connect] to `inS` with the input size.
 *   6. Composed `audio-delay` with an initial L = source period + [INITIAL_LATENCY_MARGIN_MS] (the first measurement replaces
 *      it), frame rate fps x N with `CHANGE_FRAME_RATE_ALWAYS` when `matchRefresh`.
 * - Turn off ([turnOff]):
 *   1. mpv: [FgMpvPort.disconnect].
 *   2. `FgNative.stop()` and `inS.release()`, then the flag false.
 *   3. If the presenter used the CPU path: recreate the SurfaceView Surface (toggle its visibility) before reconnecting (R4b);
 *      the new `surfaceCreated` reconnects mpv (the engine's own attach) and [onOutputSurfaceCreated] runs step 5.
 *   4. [FgMpvPort.connect] to the SurfaceView with its size.
 *   5. Restore the user audio-delay, the flag, and clear the frame-rate hint.
 * - `surfaceDestroyed` with FG active ([onOutputSurfaceDestroyed], synchronous): disconnect mpv from the input, `stop()` once,
 *   «suspended by surface», preferences untouched; [onOutputSurfaceCreated] re-enables FG with a post if still eligible
 *   (background, PiP).
 * - Hot change of N or profile: `updateConfig`; if the desired input size changes or the native side answers INVALID_ARG, an
 *   off/on cycle without reloading the file.
 *
 * The controller keeps the attach state so the port is never disconnected twice (R6): mpv is on the SurfaceView, on the FG
 * input or on nothing ([MpvTarget]).
 *
 * Other rules (unchanged from Enigmata): nothing native unless [FgNative.isAvailable] (I1); eligibility is cached and the
 * expensive facts are only read when FG is on / turning on or by [refresh] off the main thread (REV 11); DEVICE_LOST blocks
 * the file, other start failures block it after [MAX_CONSECUTIVE_START_FAILURES] (REV 10, I9); L is never persisted (I6);
 * suspensions turn FG off without changing the user switch and resume it (fact 16). Main thread only, except the stats poll
 * whose native read runs on `pollDispatcher`.
 *
 * Tatakai differences: no HDR SDR lock (the app has no HDR output modes) and no quality-controller coupling: the hybrid input
 * policy (FIX-5) asks [FgMpvPort.upscalerActive] (mpv `glsl-shaders` not empty) and re-checks it on every stats reading; with
 * FG off the frame-rate hint is cleared (the app has no refresh matching of its own); battery saver and 8K video are detected
 * here; [FgActiveFlag] is exposed for mpv option code (cadence interpolation must be off while FG runs, fact 17).
 *
 * @param scope lifetime of the poll and of the posts (the app scope).
 * @param mainDispatcher dispatcher of the posts; must post (`Dispatchers.Main`, not `.immediate`).
 * @param ioDispatcher dispatcher of [refresh] (library load and shader probe) and of the shader import.
 */
class FgController(
  private val prefs: FgPreferences,
  private val store: FrameGenSettingsStore,
  val flag: FgActiveFlag,
  private val shaderInstaller: ShaderInstaller,
  private val capabilities: FgCapabilities,
  private val scope: CoroutineScope,
  private val mainDispatcher: CoroutineDispatcher = Dispatchers.Main,
  private val pollDispatcher: CoroutineDispatcher = Dispatchers.Default,
  private val ioDispatcher: CoroutineDispatcher = Dispatchers.IO,
  private val nowMs: () -> Long = { System.nanoTime() / 1_000_000L },
) : FrameGenController {
  /** Who has mpv's video output right now (R6). */
  private enum class MpvTarget { NONE, SURFACE_VIEW, INPUT }

  private val _state = MutableStateFlow(FrameGenUiState(settings = store.settings.value))
  override val state: StateFlow<FrameGenUiState> = _state.asStateFlow()

  private var port: FgMpvPort? = null
  private var mpvTarget = MpvTarget.NONE

  // Pipeline.
  private var active = false
  private var inputSurface: Surface? = null
  private var config: FgConfig? = null

  // Amendment 3 (E17): video display size [config] was built with (null = unknown then).
  private var configVideo: Pair<Int, Int>? = null
  private var currentN = 0
  private var lastStats: FgStats? = null
  private var lastLive: FgFps? = null
  private var deviceInfo: FgDeviceInfo? = null

  // Audio compensation (L + manual offset) currently inside mpv's audio-delay. Never persisted (I6).
  private var appliedCompSec = 0.0
  private var appliedLatencyMs = 0.0
  private var lastLatencyUpdateMs = 0L

  // REV 7: false until the first valid latency measurement of this pipeline was applied.
  private var latencyMeasured = false

  // REV 16: time of the last seek-end flush (debounce of seeking=false + PLAYBACK_RESTART).
  private var lastSeekEndFlushMs: Long? = null

  // REV 11: cached expensive eligibility facts (null = not known yet / invalidated).
  private var libLoaded: Boolean? = null
  private var shadersReady: Boolean? = null
  private var shaderManifest: ShaderManifest? = null
  private var shadersGeneration = 0
  private var probeJob: Job? = null
  private var lastVulkanLevel = 0

  // Player situation.
  private var containerFps: Double? = null
  private var paused = false
  // FIX-6 (r1): `paused-for-cache` and `seeking`; like [paused], the watchdog does not count them.
  private var pausedForCache = false
  private var seeking = false
  private var inPip = false
  private var background = false
  private var lowPower = false
  private var eightK = false
  private var hdrPassthrough = false
  // Tatakai: battery saver (capabilities) and an 8K video (video-params), detected here.
  private var systemPowerSave = false
  private var videoEightK = false
  // Tatakai: the last upscaler state seen (FIX-5 hybrid input); re-checked on every stats reading.
  private var lastUpscaler = false
  // Link: the paired PC processes the current video and the phone must not add FG; only [onRemote] changes it.
  private var remote = false
  private var fileLoaded = false
  private var nativeDestroyed = false

  // Suspension / failure bookkeeping.
  private var suspendedBySurface = false
  private var awaitingRecreate = false
  private var resumeAfterRecreate = false
  private var failure: FgUnavailableReason? = null
  private var failedPath: String? = null

  // REV 10: whether [failure] blocks this file (I9), and consecutive failures on [failedPath].
  private var failureBlocks = false
  private var consecutiveFailures = 0

  // FIX-3 (M2): true once this pipeline was healthy (first stats RUNNING with outFrames > 0).
  private var sessionHealthy = false

  // FIX-3 (watchdog R1): [inputSeen] once stats.inFrames > 0 for this pipeline; until then [noInputMs] accumulates the
  // unpaused time since the start, ticked at [watchdogTickMs].
  private var inputSeen = false
  private var noInputMs = 0L
  private var watchdogTickMs = 0L

  // Amendment 2 (E9): the current [failure] came from the «no input frames» watchdog; cleared with it.
  private var noInputTimeout = false

  // Amendment 2 (E14): readings counted for the logcat line (only with `showStats`).
  private var statsLogReadings = 0

  // Amendment 3 (E19): fps measured on every stats reading, the overload state and the nominal source fps.
  private val liveMeter = FgFpsMeter(nowMs)
  private var overload = FgOverloadState()
  private var sourceFps = 0.0

  // Amendment 3 (E18): Auto budget. [autoBudgetCap] = highest N Auto may choose after a step down (null = no cap).
  private var autoBudgetCap: Int? = null
  private var budget = FgBudgetState(startedMs = 0L)

  // Amendment 3 (E17): pending deduplicated off/on cycle ([requestCycle]).
  private var cycleJob: Job? = null

  // FIX-5 (§ 13.1.5): last raw value of the input scale debug property seen ("" = unset).
  private var debugInputScaleLogged = ""

  // FIX-6 (r3): `vid` read right before mpv first connects to the FG input.
  private var vidSavedBeforeFg = false
  private var vidBeforeFg: String? = null

  /** Last computed eligibility; null = not known yet (FG off and expensive facts not probed). */
  private var eligibility: FgEligibility? = null

  private var pollJob: Job? = null
  private var resumeJob: Job? = null

  // FIX-3 (m1), Amendment 3 (E17): deferred first start of a file while its start inputs are unknown.
  private var fpsWaitJob: Job? = null

  init {
    // Amendment 1: re-evaluate when «Allow FG on untested GPUs» changes (settings, any thread).
    scope.launch(mainDispatcher) {
      prefs.allowUntestedVulkan.changes().distinctUntilChanged().drop(1).collect { reevaluateSuspension() }
    }
    // FIX-5 (§ 13.1): the hybrid input depends on the FG priority and «FG at screen resolution» (and on the upscaler, see
    // onStats); a change reconfigures a running FG.
    scope.launch(mainDispatcher) {
      combine(prefs.priority.changes(), prefs.screenResolution.changes()) { priority, screen -> priority to screen }
        .distinctUntilChanged()
        .drop(1)
        .collect { onInputPolicyChanged() }
    }
    // Tatakai: settings changed elsewhere (Settings screen, restore) are reflected in the state.
    scope.launch(mainDispatcher) { store.settings.drop(1).collect { publish() } }
  }

  // ---------------------------------------------------------------------------------------------
  // FrameGenController
  // ---------------------------------------------------------------------------------------------

  override fun setEnabled(enabled: Boolean) {
    if (enabled != prefs.enabled.get()) toggle()
  }

  /**
   * Fact 2/14: turns FG on or off hot. Off -> on only when eligible (and not failed for this file); otherwise nothing is enabled
   * and [state] carries the reason. On -> off always works. Persisted in `FrameGenSettings.enabled`. While
   * [FgEligibility.Suspended] (and not blocked by a failure) off -> on only sets the preference; FG starts when it ends.
   * Tatakai: without a bound session (Settings) off -> on only needs the device to be eligible; FG starts with the next MPV file.
   */
  fun toggle() {
    if (prefs.enabled.get()) {
      prefs.enabled.set(false)
      suspendedBySurface = false
      resumeAfterRecreate = false
      cancelResume()
      turnOff(reconnect = true)
      // REV 10: a non-blocking START_FAILED is not shown any more once the user turns FG off.
      if (!failureBlocks) failure = null
      // Amendment 2 (E9): turned off without a (kept) failure: its stats and watchdog mark go too.
      if (failure == null) resetStatsState()
      refreshEligibility()
      publish()
      return
    }
    // Turning on: the expensive facts are read now (synchronously, REV 11).
    val e = refreshEligibility(full = true)
    if (e is FgEligibility.Suspended && !failureBlocks) {
      prefs.enabled.set(true)
      publish()
      return
    }
    if (e !is FgEligibility.Eligible || failureBlocks) {
      publish()
      return
    }
    prefs.enabled.set(true)
    if (canAutoStart()) turnOn()
    publish()
  }

  override fun setMultiplier(multiplier: FrameGenMultiplier) = setMultiplier(multiplier.n)

  /** 0 = Auto, otherwise 2..5; other values are ignored. Global (fact 14), hot (fact 2). */
  fun setMultiplier(n: Int) {
    if (n != FgPreferences.MULTIPLIER_AUTO && n !in FgPreferences.MULTIPLIER_RANGE) return
    prefs.multiplier.set(n)
    autoBudgetCap = null // Amendment 3 (E18): a user choice lifts the Auto budget cap
    // FIX-9 (mi-1, I13): and resets the whole budget state (also `lastStepMs`).
    budget = FgBudgetState(startedMs = nowMs())
    if (active) applyHot()
    publish()
  }

  /** Fact 25: global profile; hot (cycle only when the input scale changes). */
  override fun setProfile(profile: FgProfile) {
    prefs.profile.set(profile)
    if (active) applyHot()
    publish()
  }

  /** Fact 25: values of [FgProfile.CUSTOM], stored bounded ([FgPolicy.resolveProfile]). */
  override fun setCustomTuning(tuning: FgTuning) {
    val bounded = FgPolicy.resolveProfile(FgProfile.CUSTOM, tuning)
    prefs.customPerformance.set(bounded.performanceMode)
    prefs.customFlowScale.set(bounded.flowScale)
    prefs.customInputScale.set(bounded.inputScale)
    if (active && prefs.profile.get() == FgProfile.CUSTOM) applyHot()
    publish()
  }

  /** Fact 8: manual offset, clamped to [FgPreferences.AUDIO_OFFSET_RANGE_MS], global; recomposes. */
  override fun setAudioOffsetMs(ms: Int) {
    val range = FgPreferences.AUDIO_OFFSET_RANGE_MS
    prefs.audioOffsetMs.set(ms.coerceIn(range.first, range.last))
    if (active) recomposeAudioDelay(appliedLatencyMs)
    publish()
  }

  /** Fact 5: with it off a running FG keeps the display mode and drops its fps x N request. */
  override fun setMatchRefresh(enabled: Boolean) {
    prefs.matchRefreshForFg.set(enabled)
    if (active) {
      if (enabled) applyFgFrameRate() else port?.clearFrameRate()
    }
    publish()
  }

  override fun setAllowUntestedGpu(allowed: Boolean) {
    prefs.allowUntestedVulkan.set(allowed) // the init collector re-evaluates
    publish()
  }

  override fun setShowStats(show: Boolean) {
    prefs.showStatsOverlay.set(show)
    publish()
  }

  override fun refresh() = requestEligibilityProbe()

  override suspend fun importShaders(uri: Uri): InstallResult {
    withContext(mainDispatcher) { stopForShaderChange() }
    val result = shaderInstaller.install(uri)
    withContext(mainDispatcher) { onShadersChanged() }
    return result
  }

  override suspend fun removeShaders(): Boolean {
    withContext(mainDispatcher) { stopForShaderChange() }
    val removed = shaderInstaller.remove()
    withContext(mainDispatcher) { onShadersChanged() }
    return removed
  }

  /** «Callers must not install while a pipeline is running on the current set»: off now, resumed by onShadersChanged. */
  private fun stopForShaderChange() {
    if (!active) return
    turnOff(reconnect = true)
    publish()
  }

  // ---------------------------------------------------------------------------------------------
  // Player wiring (MpvFrameGenBridge of :core:player-mpv)
  // ---------------------------------------------------------------------------------------------

  /**
   * Binds the port of an MPV session. mpv is taken as connected to the SurfaceView when its Surface is valid (the engine
   * attached it in `surfaceCreated`). A previous port is unbound first (a new session replaced it).
   */
  fun bind(newPort: FgMpvPort) {
    if (port != null) unbind(port!!)
    port = newPort
    nativeDestroyed = false
    mpvTarget = if (newPort.outputSurface() != null) MpvTarget.SURFACE_VIEW else MpvTarget.NONE
    lastUpscaler = newPort.upscalerActive()
    refreshEligibility()
    publish()
  }

  /** After [onNativeDestroying] (or when the session goes away). No-op unless [boundPort] is the bound one. Idempotent. */
  fun unbind(boundPort: FgMpvPort) {
    if (port == null || port !== boundPort) return
    teardown()
    port = null
    mpvTarget = MpvTarget.NONE
    fileLoaded = false
    containerFps = null
    paused = false
    pausedForCache = false
    seeking = false
    videoEightK = false
    // `remote` (Link, onRemote) and `inPip` (player UI, onPip) are kept: their owners may set them before the next session
    // binds and reset them themselves.
    publish()
  }

  /** Whether [p] is the bound port (the bridge ignores late mpv events of a replaced session). */
  fun isBound(p: FgMpvPort): Boolean = port === p

  /**
   * MPV_EVENT_FILE_LOADED. A new path clears the I9 failure of the previous file. With `enabled` and eligible FG turns on
   * (fact 14); if it is already running the queues are flushed.
   */
  fun onFileLoaded() {
    val p = port ?: return
    fileLoaded = true
    if (failedPath != null && p.currentPath() != failedPath) {
      failure = null
      failedPath = null
      failureBlocks = false
      consecutiveFailures = 0
    }
    if (failure == null) resetStatsState()
    // Amendment 3 (E18) / FIX-9 (mi-1, I13): the Auto budget belongs to the previous file.
    autoBudgetCap = null
    budget = FgBudgetState(startedMs = nowMs())
    if (active) {
      if (FgNative.isAvailable()) FgNative.flush()
    } else if (canAutoStart()) {
      if (!startInputsKnown()) {
        // FIX-3 (m1) / E17: wait for container-fps (Auto) and the video geometry up to [FPS_WAIT_MS] so the first start does
        // not need a second native start (~1 s) once they arrive.
        startAfterFpsWait()
      } else {
        turnOn()
      }
    }
    refreshEligibility()
    publish()
  }

  /** E17: the inputs of a first start are known: container-fps (only needed with Auto) and the video display size. */
  private fun startInputsKnown(): Boolean =
    (prefs.multiplier.get() != FgPreferences.MULTIPLIER_AUTO || containerFps != null) && videoSize() != null

  /** E17: a deferred first start runs now when its inputs became known. True when a wait was pending and was resolved. */
  private fun resolveStartWait(): Boolean {
    if (fpsWaitJob?.isActive != true || !startInputsKnown()) return false
    cancelFpsWait()
    if (canAutoStart() && refreshEligibility() is FgEligibility.Eligible) turnOn()
    publish()
    return true
  }

  /** FIX-3 (m1): posts the deferred start of [onFileLoaded] (deduplicated). */
  private fun startAfterFpsWait() {
    if (fpsWaitJob?.isActive == true) return
    fpsWaitJob = scope.launch(mainDispatcher) {
      delay(FPS_WAIT_MS)
      fpsWaitJob = null
      if (canAutoStart() && refreshEligibility() is FgEligibility.Eligible) turnOn()
      publish()
    }
  }

  private fun cancelFpsWait() {
    fpsWaitJob?.cancel()
    fpsWaitJob = null
  }

  /** `pause` observer (I5). */
  fun onPause(paused: Boolean) {
    // FIX-3 (watchdog): close the current interval with the old pause state.
    if (active) tickWatchdog()
    this.paused = paused
    if (active && FgNative.isAvailable()) FgNative.setPaused(paused)
  }

  /** FIX-6 (r1), `paused-for-cache` observer: that time is not counted by the watchdog. */
  fun onPausedForCache(pausedForCache: Boolean) {
    if (active) tickWatchdog()
    this.pausedForCache = pausedForCache
  }

  /**
   * `seeking` observer: a seek start is a discontinuity (I4, fact 15); its end (`seeking=false`) flushes again through
   * [onSeekFinished] (REV 16). FIX-6 (r2): paused, no flush at all (see [onSeek]).
   */
  fun onSeeking(seeking: Boolean) {
    if (active) tickWatchdog()
    this.seeking = seeking
    if (seeking) {
      lastSeekEndFlushMs = null
      if (active && !paused && FgNative.isAvailable()) FgNative.flush()
    } else {
      onSeekFinished()
    }
  }

  /**
   * MPV_EVENT_SEEK: flush (I4, fact 15). FIX-6 (r2): not while paused; the engine's `presentPausedFrame` (fg_engine.cpp)
   * already forces the bootstrap on the paused frame.
   */
  fun onSeek() {
    lastSeekEndFlushMs = null
    if (active && !paused && FgNative.isAvailable()) FgNative.flush()
  }

  /** `seeking=false` and MPV_EVENT_PLAYBACK_RESTART (REV 16): flush once more after the seek (debounced). */
  fun onSeekFinished() {
    if (!active) return
    if (paused) return
    val now = nowMs()
    val last = lastSeekEndFlushMs
    if (last != null && now - last in 0 until SEEK_END_FLUSH_DEBOUNCE_MS) return
    lastSeekEndFlushMs = now
    if (FgNative.isAvailable()) FgNative.flush()
  }

  /**
   * `container-fps`: source period; with Auto the multiplier is recomputed (updateConfig) and the fps x N frame rate
   * re-applied. FIX-5: with any N the hybrid input size is re-checked.
   */
  fun onContainerFps(fps: Double) {
    containerFps = fps.takeIf { it.isFinite() && it > 0.0 }
    if (!active) {
      resolveStartWait()
      return
    }
    val period = sourcePeriodNs(containerFps)
    if (period > 0 && FgNative.isAvailable()) FgNative.setSourcePeriodNs(period)
    applyHot()
    applyFgFrameRate()
    publish()
  }

  /**
   * E17: `video-params` observers. While FG is off it resolves a pending first-start wait; while it is on and the desired
   * input size no longer matches the running config, one deduplicated off/on cycle. Tatakai: also the 8K suspension.
   */
  fun onVideoGeometryChanged() {
    if (port == null) return
    val eight = isEightK(videoSize())
    if (eight != videoEightK) {
      videoEightK = eight
      reevaluateSuspension()
    }
    if (!active) {
      resolveStartWait()
      return
    }
    val cfg = config ?: return
    if (videoSize() == null) return
    val desired = desiredInputSize(cfg.outputWidth, cfg.outputHeight, currentTuning())
    if (desired.width != cfg.inputWidth || desired.height != cfg.inputHeight) {
      if (requestCycle() && prefs.showStatsOverlay.get()) {
        Log.i(TAG, "Video geometry changed: input ${cfg.inputWidth}x${cfg.inputHeight} -> ${desired.width}x${desired.height}, cycling")
      }
    }
  }

  /**
   * FIX-5: the upscaler, the FG priority or «FG at screen resolution» changed. Like [onVideoGeometryChanged]: a cycle when the
   * desired input size changed; with the same size, a new effective multiplier is applied hot.
   */
  private fun onInputPolicyChanged() {
    if (port == null || !active) return
    val cfg = config ?: return
    val desired = desiredInputSize(cfg.outputWidth, cfg.outputHeight, currentTuning(), videoSize() ?: configVideo)
    if (desired.width != cfg.inputWidth || desired.height != cfg.inputHeight) {
      if (requestCycle() && prefs.showStatsOverlay.get()) {
        Log.i(TAG, "FG input policy changed: input ${cfg.inputWidth}x${cfg.inputHeight} -> ${desired.width}x${desired.height}, cycling")
      }
    } else if (desired.effectiveN != currentN) {
      applyHot()
      publish()
    }
  }

  override fun onPip(inPip: Boolean) {
    this.inPip = inPip
    reevaluateSuspension()
  }

  /** The player went to / came back from the background (fact 16). The surface lifecycle already covers most cases. */
  fun onBackground(background: Boolean) {
    this.background = background
    reevaluateSuspension()
  }

  override fun onRemote(active: Boolean, phoneFgAllowed: Boolean) {
    this.remote = active && !phoneFgAllowed
    reevaluateSuspension()
  }

  override fun onLowPower(lowPower: Boolean, eightK: Boolean) {
    this.lowPower = lowPower
    this.eightK = eightK
    reevaluateSuspension()
  }

  override fun onHdrPassthrough(passthrough: Boolean) {
    hdrPassthrough = passthrough
    reevaluateSuspension()
  }

  /**
   * REV 3 / I11: a renderer failure of mpv while [FgActiveFlag] was true is an FG failure: turn FG off restoring everything,
   * with START_FAILED (REV 10 counting). No-op when FG is not active any more.
   */
  fun onRendererFailure() {
    val p = port ?: return
    if (!active) return
    Log.w(TAG, "mpv renderer failure while FG was active: turning FG off")
    failAndStop(p, FgUnavailableReason.START_FAILED)
  }

  /** REV 11: the installed shader set changed (import finished or set removed); re-probe it off the main thread. */
  fun onShadersChanged() {
    shadersReady = null
    shaderManifest = null
    shadersGeneration++
    probeJob?.cancel()
    probeJob = null
    refreshEligibility()
    publish()
    requestEligibilityProbe()
  }

  /**
   * REV 11: reads the expensive eligibility facts ([FgNative.isAvailable], which loads the library, and
   * [ShaderInstaller.isReady]) on [ioDispatcher] when they are not cached, then re-evaluates and publishes on the main thread.
   * Idempotent while a probe runs.
   */
  fun requestEligibilityProbe() {
    if (libLoaded != null && shadersReady != null) return
    if (probeJob?.isActive == true) return
    val generation = shadersGeneration
    probeJob = scope.launch(mainDispatcher) {
      val (lib, ready, manifest) = withContext(ioDispatcher) {
        val available = FgNative.isAvailable()
        Triple(available, available && shaderInstaller.isReady(), shaderInstaller.manifest())
      }
      probeJob = null
      if (libLoaded == null) libLoaded = lib
      if (shadersReady == null && generation == shadersGeneration) {
        shadersReady = ready
        shaderManifest = manifest
      }
      reevaluateSuspension()
    }
  }

  /**
   * `surfaceCreated`, called **after** the engine attached mpv to the new SurfaceView Surface. Completes an R4b recreation
   * (step 5 of turn off) and posts a re-enable when FG was suspended by the surface or is enabled and eligible.
   */
  fun onOutputSurfaceCreated() {
    val p = port ?: return
    mpvTarget = MpvTarget.SURFACE_VIEW
    if (awaitingRecreate) {
      awaitingRecreate = false
      // The engine's surfaceCreated already reconnected mpv (attach, force-window, vo) and surfaceChanged sets its size.
      restoreAfterFg(p)
      restoreVidAfterFailure(p) // FIX-3 (m4)
      resumeAfterRecreate = false
      requestResume()
      publish()
      return
    }
    if (suspendedBySurface || prefs.enabled.get()) requestResume()
  }

  /**
   * `surfaceChanged`, called **before** the engine's resize. True when the controller handled it and the engine must NOT
   * resize (FG active: mpv renders into the FG input, whose `android-surface-size` must stay the input size). A new output
   * size needs a stop/start cycle (posted, deduplicated with the video geometry one).
   */
  fun onOutputSurfaceChanged(width: Int, height: Int): Boolean {
    if (!active) return false
    val cfg = config ?: return true
    if (width > 0 && height > 0 && (width != cfg.outputWidth || height != cfg.outputHeight)) requestCycle()
    return true
  }

  /**
   * `surfaceDestroyed`, called **before** the engine's detach, while the Surface is still valid. True when the engine must NOT
   * detach (mpv is not on the SurfaceView: a second detach, R6).
   * - FG active: disconnect mpv from the input, `stop()` once, «suspended by surface»; preferences untouched.
   * - R4b recreation in progress: posts the view visible again.
   * - Otherwise false; the engine detaches mpv from the SurfaceView.
   */
  fun onOutputSurfaceDestroyed(): Boolean {
    val p = port ?: return false
    p.clearFrameRate()
    if (active) {
      stopPoll()
      disconnectMpv(p)
      stopNative()
      active = false
      suspendedBySurface = true
      restoreAfterFg(p)
      publish()
      return true
    }
    if (awaitingRecreate) {
      post { port?.setOutputVisible(true) }
      return true
    }
    mpvTarget = MpvTarget.NONE
    return false
  }

  /** Synchronous, right before `MPVLib.destroy`: stops and cleans up without reconnecting mpv; nothing is posted afterwards. */
  fun onNativeDestroying() {
    teardown()
    nativeDestroyed = true
    fileLoaded = false
    publish()
  }

  override fun userAudioDelaySec(effective: Double): Double = FgAudioDelay.userFrom(effective, appliedCompSec)

  override fun effectiveAudioDelaySec(user: Double): Double = user + appliedCompSec

  /** Frame rate the SurfaceView should request while FG is active (fps x N, `alwaysChange`); null with FG off. */
  fun frameRateFor(containerFps: Double): Double? {
    if (!active) return null
    return FgPolicy.targetFrameRate(containerFps, currentN, prefs.matchRefreshForFg.get())
  }

  // ---------------------------------------------------------------------------------------------
  // Turn on / turn off
  // ---------------------------------------------------------------------------------------------

  /** Turn on, in the literal order of the design. Returns true when FG is active afterwards. */
  private fun turnOn(): Boolean {
    val p = port ?: return false
    if (active) return true
    if (awaitingRecreate || nativeDestroyed || failureBlocks) return false
    cancelFpsWait()

    // 1. Eligibility (with the native probe, only now that FG is about to start).
    probeDeviceIfNeeded()
    val e = refreshEligibility(full = true)
    if (e !is FgEligibility.Eligible) {
      publish()
      return false
    }
    val out = p.outputSurface() ?: return false
    val (outW, outH) = p.outputSize() ?: return false
    lastUpscaler = p.upscalerActive()
    val tuning = currentTuning()
    val video = videoSize()
    val plan = desiredInputSize(outW, outH, tuning, video)
    val inW = plan.width
    val inH = plan.height
    val n = plan.effectiveN
    currentN = n
    val userDelay = FgAudioDelay.userFrom(p.getAudioDelaySec() ?: 0.0, appliedCompSec)
    resetStatsState()

    // 2. inS = createInputSurface(inputSize).
    val inS = FgNative.createInputSurface(inW, inH)
    if (inS == null) {
      FgNative.stop() // releases a reader opened by a failed call, if any (idempotent)
      failForFile(p, FgUnavailableReason.START_FAILED)
      publish()
      return false
    }

    // 3. mpv off the SurfaceView.
    disconnectMpv(p)

    // 4. start(SurfaceView surface, cfg).
    val cfg = FgConfig(
      multiplier = n,
      performanceMode = tuning.performanceMode,
      flowScale = tuning.flowScale,
      inputWidth = inW,
      inputHeight = inH,
      outputWidth = outW,
      outputHeight = outH,
      allowFp16 = deviceInfo?.fp16 == true,
      antiArtifacts = true,
      verbose = false,
      shaderDir = shaderInstaller.shaderDir().absolutePath,
    )
    val startNs = System.nanoTime()
    val code = FgNative.start(out, cfg)
    val startMs = (System.nanoTime() - startNs) / 1_000_000L
    if (startMs > START_LOG_THRESHOLD_MS) Log.i(TAG, "FgNative.start took $startMs ms (code $code)")
    if (code != FgResult.OK.code) {
      FgNative.stop() // after a failed start the reader stays open
      inS.release()
      flag.set(false)
      connectMpv(p, out, MpvTarget.SURFACE_VIEW, outW, outH)
      if (FgResult.fromCode(code) == FgResult.SHADERS_MISSING) shadersReady = null
      failForFile(p, reasonFor(code))
      publish()
      return false
    }

    // 5. FgActiveFlag=true right before mpv opens its VO on the input (I11), then mpv on the input.
    vidBeforeFg = p.getPropertyString("vid")
    vidSavedBeforeFg = true
    flag.set(true)
    try {
      connectMpv(p, inS, MpvTarget.INPUT, inW, inH)
    } catch (t: RuntimeException) {
      // FIX-6 (r7): never leave the flag set nor the pipeline running; mpv is not reconnected (unknown attach state, R6).
      flag.set(false)
      FgNative.stop()
      inS.release()
      throw t
    }
    inputSurface = inS
    config = cfg
    configVideo = video
    currentN = n
    active = true
    budget = FgBudgetState(startedMs = nowMs(), lastStepMs = budget.lastStepMs)
    suspendedBySurface = false
    lastStats = null
    lastLive = null
    if (!failureBlocks) failure = null
    sessionHealthy = false
    inputSeen = false
    noInputMs = 0L
    watchdogTickMs = nowMs()

    // 6. Composed audio-delay (fact 8). REV 7: initial L = source period + margin unless already measured.
    val startStats = FgStats.from(FgNative.stats())
    latencyMeasured = startStats.latencyNs > 0
    val latencyMs = if (latencyMeasured) {
      quantizeLatency(startStats.latencyNs / NS_PER_MS)
    } else {
      initialLatencyMs(sourcePeriodNs(containerFps).takeIf { it > 0 } ?: startStats.periodNs)
    }
    appliedLatencyMs = latencyMs
    lastLatencyUpdateMs = nowMs()
    val offset = prefs.audioOffsetMs.get()
    p.setAudioDelaySec(FgAudioDelay.compose(userDelay, latencyMs, offset))
    appliedCompSec = FgAudioDelay.compensationSec(latencyMs, offset)
    //    Frame rate fps x N, ALWAYS (fact 5).
    applyFgFrameRate()

    val period = sourcePeriodNs(containerFps)
    if (period > 0) FgNative.setSourcePeriodNs(period)
    if (paused) FgNative.setPaused(true)
    Log.i(
      TAG,
      String.format(
        Locale.US,
        "FG input: in=%dx%d out=%dx%d N=%d%s upscaler=%b priority=%s screenRes=%b gpu=%s scale=%.2f",
        inW,
        inH,
        outW,
        outH,
        n,
        inputRatioLog(),
        lastUpscaler,
        prefs.priority.get().name,
        prefs.screenResolution.get(),
        FgPolicy.gpuClassOf(deviceInfo?.gpuName).name,
        debugInputScale() ?: tuning.inputScale,
      ),
    )
    startPoll()
    publish()
    return true
  }

  /** Turn off, in the literal order of the design. [reconnect] = false (native core going away) skips steps 3-4. */
  private fun turnOff(reconnect: Boolean) {
    val p = port ?: return
    if (!active) return
    stopPoll()

    // 1. mpv off the input.
    disconnectMpv(p)

    // 2. FgNative.stop() and inS.release().
    val cpu = stopNative()
    active = false
    if (failure == null) resetStatsState()
    flag.set(false)

    // 3. presentPath was CPU: recreate the SurfaceView Surface before reconnecting (R4b).
    if (reconnect && cpu && p.outputSurface() != null) {
      awaitingRecreate = true
      p.setOutputVisible(false)
      publish()
      return
    }

    // 4. mpv back on the SurfaceView with its size.
    if (reconnect) {
      val out = p.outputSurface()
      val size = p.outputSize()
      if (out != null && size != null) {
        connectMpv(p, out, MpvTarget.SURFACE_VIEW, size.first, size.second)
        restoreVidAfterFailure(p)
      }
    }

    // 5. User audio-delay, flag, frame-rate hint.
    restoreAfterFg(p)
    publish()
  }

  /** Turn off step 2: `stop()` (blocking, idempotent) and `inS.release()`. Returns whether the presenter used the CPU path. */
  private fun stopNative(): Boolean {
    var cpu = lastStats?.presentPath == PresentPath.CPU
    if (FgNative.isAvailable()) {
      FgNative.stop()
      val after = FgStats.from(FgNative.stats())
      cpu = cpu || after.presentPath == PresentPath.CPU
      lastStats = after
    }
    inputSurface?.release()
    inputSurface = null
    config = null
    configVideo = null
    return cpu
  }

  /** Turn off step 5. Audio first (needs [appliedCompSec]), then the flag and the frame-rate hint. */
  private fun restoreAfterFg(p: FgMpvPort) {
    if (appliedCompSec != 0.0) {
      p.getAudioDelaySec()?.let { p.setAudioDelaySec(FgAudioDelay.userFrom(it, appliedCompSec)) }
    }
    appliedCompSec = 0.0
    appliedLatencyMs = 0.0
    latencyMeasured = false
    flag.set(false)
    // Tatakai: the app requests no refresh rate of its own, so FG's fps x N request is simply dropped.
    p.clearFrameRate()
  }

  /** FIX-3 (m4) / FIX-6 (r3): after a failure, reselect the video track if FG made mpv drop it (`vid=no`). */
  private fun restoreVidAfterFailure(p: FgMpvPort) {
    if (failure == null) return
    if (!vidSavedBeforeFg || vidBeforeFg == "no") return
    if (p.getPropertyString("vid") == "no") p.setPropertyString("vid", "auto")
  }

  /** fps x N with CHANGE_FRAME_RATE_ALWAYS while active and `matchRefresh`; else no-op. */
  private fun applyFgFrameRate() {
    val p = port ?: return
    if (!active || !prefs.matchRefreshForFg.get()) return
    FgPolicy.targetFrameRate(containerFps, currentN, matchRefresh = true)?.let { p.applyFrameRate(it, alwaysChange = true) }
  }

  /** Off/on cycle without reloading the file (input size or output size change). */
  private fun cycle() {
    turnOff(reconnect = true)
    if (awaitingRecreate) resumeAfterRecreate = true else turnOn()
  }

  /** E17 / FIX-9 / FIX-12: posts one deduplicated off/on cycle; true only when this call scheduled it. */
  private fun requestCycle(): Boolean {
    if (cycleJob?.isActive == true) return false
    cycleJob = post {
      cycleJob = null
      if (active && cycleStillNeeded()) cycle()
      publish()
    }
    return true
  }

  /** FIX-9 (mi-2): the running config no longer matches the desired input or the output size. */
  private fun cycleStillNeeded(): Boolean {
    val cfg = config ?: return false
    val desired = desiredInputSize(cfg.outputWidth, cfg.outputHeight, currentTuning(), videoSize() ?: configVideo)
    if (desired.width != cfg.inputWidth || desired.height != cfg.inputHeight) return true
    val out = port?.outputSize() ?: return false
    return out.first != cfg.outputWidth || out.second != cfg.outputHeight
  }

  private fun cancelCycle() {
    cycleJob?.cancel()
    cycleJob = null
  }

  /** Hot change: updateConfig, or a cycle when the desired input size differs from the running config / is rejected. */
  private fun applyHot() {
    val cfg = config ?: return
    val tuning = currentTuning()
    val desired = desiredInputSize(cfg.outputWidth, cfg.outputHeight, tuning, videoSize() ?: configVideo)
    if (desired.width != cfg.inputWidth || desired.height != cfg.inputHeight) {
      cycle()
      return
    }
    val n = desired.effectiveN
    val next = cfg.copy(multiplier = n, performanceMode = tuning.performanceMode, flowScale = tuning.flowScale)
    if (next == cfg) return
    val code = if (FgNative.isAvailable()) FgNative.updateConfig(next) else FgResult.NOT_INITIALIZED.code
    when (FgResult.fromCode(code)) {
      FgResult.OK -> {
        config = next
        if (n != currentN) {
          currentN = n
          applyFgFrameRate()
        }
      }
      FgResult.INVALID_ARG -> cycle()
      else -> port?.let { failAndStop(it, reasonFor(code)) }
    }
  }

  /** Detaches mpv from whatever it is on, at most once (R6). */
  private fun disconnectMpv(p: FgMpvPort) {
    if (mpvTarget == MpvTarget.NONE) return
    p.disconnect()
    mpvTarget = MpvTarget.NONE
  }

  /** Attaches mpv to [surface] (never while attached, R6). */
  private fun connectMpv(p: FgMpvPort, surface: Surface, target: MpvTarget, width: Int, height: Int) {
    if (mpvTarget != MpvTarget.NONE) disconnectMpv(p)
    p.connect(surface, width, height)
    mpvTarget = target
  }

  /** Stop and clean up without reconnecting (native core going away or unbind). */
  private fun teardown() {
    cancelResume()
    cancelCycle()
    val p = port
    if (p != null && active) turnOff(reconnect = false)
    if (p != null && awaitingRecreate) {
      awaitingRecreate = false
      resumeAfterRecreate = false
      restoreAfterFg(p)
      p.setOutputVisible(true)
    }
    stopPoll()
    suspendedBySurface = false
  }

  // ---------------------------------------------------------------------------------------------
  // Failure (I9), suspension (fact 16) and latency (fact 8)
  // ---------------------------------------------------------------------------------------------

  /** REV 10: records a failure for the current file (DEVICE_LOST blocks it at once, START_FAILED after a run of them). */
  private fun failForFile(p: FgMpvPort, reason: FgUnavailableReason) {
    val path = p.currentPath()
    if (path != failedPath) {
      consecutiveFailures = 0
      failureBlocks = false
    }
    failedPath = path
    consecutiveFailures++
    failure = reason
    failureBlocks = failureBlocks || reason == FgUnavailableReason.DEVICE_LOST ||
      consecutiveFailures >= MAX_CONSECUTIVE_START_FAILURES
  }

  /** I9: turn off restoring everything and keep the reason. */
  private fun failAndStop(p: FgMpvPort, reason: FgUnavailableReason) {
    failForFile(p, reason)
    turnOff(reconnect = true)
    publish()
    if (prefs.showStatsOverlay.get()) {
      val view = FgStatsView.from(_state.value, null)
      Log.i(TAG, "FG failed: reason=${reason.name} noInputTimeout=$noInputTimeout; ${view.logLine()}")
    }
  }

  /** E9/E14/E19: forgets the stats, the watchdog mark, the logcat readings, the fps meter and the overload state. */
  private fun resetStatsState() {
    lastStats = null
    lastLive = null
    noInputTimeout = false
    statsLogReadings = 0
    liveMeter.reset()
    overload = FgOverloadState()
    sourceFps = 0.0
  }

  /** REV 10: native code -> reason. Only DEVICE_LOST is a lost device; every other failure is START_FAILED. */
  private fun reasonFor(code: Int): FgUnavailableReason =
    if (FgResult.fromCode(code) == FgResult.DEVICE_LOST) FgUnavailableReason.DEVICE_LOST else FgUnavailableReason.START_FAILED

  private fun reevaluateSuspension() {
    val e = refreshEligibility()
    if (active && e !is FgEligibility.Eligible) {
      turnOff(reconnect = true) // the user switch is unchanged (fact 16)
    } else if (!active && e is FgEligibility.Eligible && prefs.enabled.get()) {
      requestResume()
    }
    publish()
  }

  private fun canAutoStart(): Boolean =
    port != null && fileLoaded && !nativeDestroyed && !active && !awaitingRecreate && !failureBlocks && prefs.enabled.get()

  /** Posts one re-enable (deduplicated); never after a turn off caused by a failure (FIX-3, M2). */
  private fun requestResume() {
    if (failure != null) return
    if (resumeJob?.isActive == true) return
    resumeJob = post {
      resumeJob = null
      if (failure == null && canAutoStart() && refreshEligibility() is FgEligibility.Eligible) turnOn()
      publish()
    }
  }

  /** Cancels a pending re-enable and (FIX-3, m1) a pending fps wait. */
  private fun cancelResume() {
    resumeJob?.cancel()
    resumeJob = null
    cancelFpsWait()
  }

  private fun startPoll() {
    stopPoll()
    pollJob = scope.launch(mainDispatcher) {
      while (isActive) {
        delay(POLL_INTERVAL_MS)
        if (!active) break
        val raw = withContext(pollDispatcher) { FgNative.stats() }
        if (!active) break
        onStats(FgStats.from(raw))
      }
    }
  }

  private fun stopPoll() {
    pollJob?.cancel()
    pollJob = null
  }

  /** Main thread: one stats snapshot while active. */
  private fun onStats(s: FgStats) {
    val p = port ?: return
    lastStats = s
    if (FgResult.fromCode(s.lastError) == FgResult.DEVICE_LOST) {
      failAndStop(p, FgUnavailableReason.DEVICE_LOST)
      return
    }
    if (s.state == FgState.FAILED) {
      failAndStop(p, FgUnavailableReason.START_FAILED)
      return
    }
    // FIX-3 (watchdog R1): mpv never delivered a frame to the input within [WATCHDOG_NO_INPUT_MS] of unpaused time.
    if (!inputSeen) {
      if (s.inFrames > 0) {
        inputSeen = true
      } else {
        tickWatchdog()
        if (noInputMs >= WATCHDOG_NO_INPUT_MS) {
          Log.w(TAG, "No input frames $noInputMs ms after start: treating it as a start failure")
          noInputTimeout = true
          failAndStop(p, FgUnavailableReason.START_FAILED)
          return
        }
      }
    }
    if (!sessionHealthy && s.state == FgState.RUNNING && s.outFrames > 0) {
      sessionHealthy = true
      if (!failureBlocks) consecutiveFailures = 0
    }
    if (s.latencyNs > 0) {
      val measured = s.latencyNs / NS_PER_MS
      if (!latencyMeasured) {
        latencyMeasured = true
        val quantized = quantizeLatency(measured)
        if (quantized != appliedLatencyMs) recomposeAudioDelay(quantized)
        lastLatencyUpdateMs = nowMs()
      } else if (FgPolicy.shouldUpdateLatency(appliedLatencyMs, measured, nowMs() - lastLatencyUpdateMs)) {
        recomposeAudioDelay(quantizeLatency(measured))
        lastLatencyUpdateMs = nowMs()
      }
    }
    val counting = !paused && !pausedForCache && !seeking && s.state == FgState.RUNNING
    val periodNs = sourcePeriodNs(containerFps).takeIf { it > 0 } ?: s.periodNs
    sourceFps = if (periodNs > 0) NS_PER_S / periodNs else 0.0
    val live = liveMeter.update(s)
    if (live != null) lastLive = live
    val speed = port?.getPropertyDouble("speed")?.takeIf { it > 0.0 } ?: 1.0
    val expectedFps = (sourceFps * speed).takeIf { it > 0.0 }
    overload = FgPolicy.overloadStep(overload, nowMs(), live?.inFps, expectedFps, counting)
    val budgetBefore = budget
    val step = FgPolicy.autoBudgetStep(
      budget,
      FgBudgetSample(
        nowMs = nowMs(),
        auto = prefs.multiplier.get() == FgPreferences.MULTIPLIER_AUTO,
        n = currentN,
        counting = counting,
        outFrames = s.outFrames,
        droppedLate = s.droppedLate,
        computeNsAvg = s.computeNsAvg,
        sourcePeriodNs = periodNs,
      ),
    )
    budget = step.state
    val stepTo = step.stepTo
    if (stepTo != null) {
      val from = currentN
      autoBudgetCap = stepTo
      Log.i(
        TAG,
        String.format(
          Locale.US,
          "Auto budget: N %d -> %d (compute=%.2fms period=%.2fms late=%d/%d)",
          from,
          stepTo,
          s.computeNsAvg / NS_PER_MS,
          periodNs / NS_PER_MS,
          s.droppedLate - budgetBefore.windowDropped,
          s.outFrames - budgetBefore.windowOutFrames,
        ),
      )
      applyHot()
      publish()
      if (!active || lastStats !== s) return
    }
    // Tatakai: the upscaler chain may change during playback (preset governor, in-player quality menu): FIX-5 re-plan.
    val upscaler = p.upscalerActive()
    if (upscaler != lastUpscaler) {
      lastUpscaler = upscaler
      onInputPolicyChanged()
      if (!active || lastStats !== s) return
    }
    // Tatakai: battery saver turned on during playback suspends FG (checked on this existing reading, no new poll).
    if (capabilities.powerSaveMode() != systemPowerSave) {
      reevaluateSuspension()
      if (!active) return
    }
    _state.update {
      it.copy(
        stats = s,
        audioCompMs = appliedCompSec * MS_PER_S,
        refreshHz = capabilities.currentHz(),
        overloaded = overload.overloaded,
        measuredInFps = overload.inFps,
        sourceFps = sourceFps,
        liveFps = lastLive,
      )
    }
    if (prefs.showStatsOverlay.get()) {
      statsLogReadings++
      if (statsLogReadings % STATS_LOG_EVERY_READINGS == 0) {
        Log.i(TAG, FgStatsView.from(_state.value, live).logLine() + inputRatioLog())
      }
    } else if (statsLogReadings != 0) {
      statsLogReadings = 0
    }
  }

  /** FIX-3 / FIX-6 (r1): adds the time since the last tick to [noInputMs] unless paused, waiting for the cache or seeking. */
  private fun tickWatchdog() {
    val now = nowMs()
    val counting = !inputSeen && !paused && !pausedForCache && !seeking
    if (counting) noInputMs += (now - watchdogTickMs).coerceAtLeast(0L)
    watchdogTickMs = now
  }

  /** Rewrites audio-delay = user + [latencyMs] + offset, keeping the user part (I6). */
  private fun recomposeAudioDelay(latencyMs: Double) {
    val p = port ?: return
    val effective = p.getAudioDelaySec() ?: return
    val user = FgAudioDelay.userFrom(effective, appliedCompSec)
    val offset = prefs.audioOffsetMs.get()
    p.setAudioDelaySec(FgAudioDelay.compose(user, latencyMs, offset))
    appliedCompSec = FgAudioDelay.compensationSec(latencyMs, offset)
    appliedLatencyMs = latencyMs
  }

  // ---------------------------------------------------------------------------------------------
  // Helpers
  // ---------------------------------------------------------------------------------------------

  /** Runs the native Vulkan probe once, only while no pipeline runs (deviceInfo is null then). */
  private fun probeDeviceIfNeeded() {
    if (deviceInfo != null || active || !FgNative.isAvailable()) return
    deviceInfo = FgNative.deviceInfo()
  }

  /**
   * Current eligibility (Amendment 1). With [full] (FG on, enabled or turning on) the expensive facts are read synchronously
   * when not cached; without it only cached facts or a cheap verdict are used (REV 11).
   */
  private fun refreshEligibility(full: Boolean = active || (prefs.enabled.get() && port != null)): FgEligibility? {
    if (full) {
      if (libLoaded == null) libLoaded = FgNative.isAvailable()
      if (shadersReady == null) {
        shadersReady = libLoaded == true && shaderInstaller.isReady()
        shaderManifest = shaderInstaller.manifest()
      }
    }
    systemPowerSave = capabilities.powerSaveMode()
    val lib = libLoaded
    val ready = shadersReady
    val info = deviceInfo
    val level = if (info == null || (info.ahb && info.swapchain)) capabilities.vulkanLevel(info) else 0
    lastVulkanLevel = level
    val known = lib != null && ready != null
    val e = FgPolicy.eligibility(
      abiOk = capabilities.abiOk,
      libLoaded = lib ?: true,
      vulkanLevel = level,
      allowUntested = prefs.allowUntestedVulkan.get(),
      gpuName = info?.gpuName,
      shadersReady = if (lib == false) false else ready ?: true,
      inPip = inPip,
      background = background,
      lowPower = lowPower || systemPowerSave,
      eightK = eightK || videoEightK,
      hdrPassthrough = hdrPassthrough,
      remote = remote,
    ).takeIf { known || it !is FgEligibility.Eligible }
    eligibility = e
    return e
  }

  /** The preference, or Auto (fact 4; E18: capped on experimental GPUs and by the Auto budget). */
  private fun resolveMultiplier(budgetCap: Boolean = true): Int {
    val pref = prefs.multiplier.get()
    if (pref in FgPreferences.MULTIPLIER_RANGE) return pref
    var maxN = if (isExperimentalNow()) FgPolicy.AUTO_EXPERIMENTAL_MAX_MULTIPLIER else FgPolicy.MAX_MULTIPLIER
    if (budgetCap) autoBudgetCap?.let { maxN = min(maxN, it) }
    return FgPolicy.autoMultiplier(
      fps = containerFps ?: Double.NaN,
      currentHz = capabilities.currentHz(),
      supportedHz = capabilities.supportedHz(),
      allowModeSwitch = prefs.matchRefreshForFg.get(),
      maxN = maxN,
    ).n
  }

  private fun isExperimentalNow(): Boolean =
    when (val e = eligibility) {
      is FgEligibility.Eligible -> e.experimental
      else -> isExperimental(lastVulkanLevel, deviceInfo)
    }

  /** E17: video display size from `video-params` (dw×dh, swapped for rotate 90/270); null when unknown. */
  private fun videoSize(): Pair<Int, Int>? {
    val p = port ?: return null
    return FgPolicy.videoDisplaySize(
      p.getPropertyInt("video-params/dw"),
      p.getPropertyInt("video-params/dh"),
      p.getPropertyInt("video-params/rotate"),
    )
  }

  /** E17 / FIX-5: FG input size and effective multiplier for an output of [outW]×[outH] ([FgPolicy.inputSizeHybrid]). */
  private fun desiredInputSize(outW: Int, outH: Int, tuning: FgTuning, video: Pair<Int, Int>? = videoSize()): FgInputPlan {
    val plan = FgPolicy.inputSizeHybrid(
      outW = outW,
      outH = outH,
      scale = debugInputScale() ?: tuning.inputScale,
      video = video,
      fps = containerFps,
      n = resolveMultiplier(budgetCap = false),
      priority = prefs.priority.get(),
      gpuClass = FgPolicy.gpuClassOf(deviceInfo?.gpuName),
      screenResolution = prefs.screenResolution.get(),
      qualityActive = lastUpscaler,
      floor = FgPolicy.snapVideoFitFloor(prefs.videoFitFloor.get()).toDouble(),
      performanceMode = tuning.performanceMode,
    )
    return plan.copy(effectiveN = min(plan.effectiveN, resolveMultiplier()))
  }

  /** FIX-5 (§ 13.1.5): `debug.tatakai.fg.input_scale` (measurements over adb) replaces the profile input scale. */
  private fun debugInputScale(): Float? {
    val raw = readSystemProperty(DEBUG_INPUT_SCALE_PROPERTY)?.trim().orEmpty()
    val value = raw.toFloatOrNull()?.takeIf { it.isFinite() && it in DEBUG_INPUT_SCALE_MIN..DEBUG_INPUT_SCALE_MAX }
    if (raw != debugInputScaleLogged) {
      debugInputScaleLogged = raw
      when {
        value != null -> Log.i(TAG, "$DEBUG_INPUT_SCALE_PROPERTY=$raw replaces the profile input scale")
        raw.isNotEmpty() -> Log.w(TAG, "$DEBUG_INPUT_SCALE_PROPERTY=$raw ignored (valid: 0.25..1.0)")
      }
    }
    return value
  }

  /** FIX-5 (§ 13.4): ` video=WxH inRatio=r` of the running config for the logcat lines. */
  private fun inputRatioLog(): String {
    val cfg = config ?: return ""
    val video = configVideo ?: return " video=? inRatio=?"
    val ratio = min(cfg.inputWidth.toDouble() / video.first, cfg.inputHeight.toDouble() / video.second)
    return String.format(Locale.US, " video=%dx%d inRatio=%.2f", video.first, video.second, ratio)
  }

  private fun currentTuning(): FgTuning =
    FgPolicy.resolveProfile(
      prefs.profile.get(),
      FgTuning(
        performanceMode = prefs.customPerformance.get(),
        flowScale = prefs.customFlowScale.get(),
        inputScale = prefs.customInputScale.get(),
      ),
    )

  private fun post(block: () -> Unit): Job = scope.launch(mainDispatcher) { block() }

  private fun publish() {
    val e = eligibility
    val multiplierPref = prefs.multiplier.get()
    _state.value = FrameGenUiState(
      enabled = prefs.enabled.get(),
      active = active,
      // E23 / FIX-9 (MA-1): the effective N while active, and after a failure with Auto; else the preference.
      multiplier = if (
        active || (multiplierPref == FgPreferences.MULTIPLIER_AUTO && failure != null && currentN in FgPreferences.MULTIPLIER_RANGE)
      ) {
        currentN
      } else {
        multiplierPref
      },
      auto = multiplierPref == FgPreferences.MULTIPLIER_AUTO,
      unavailable = failure ?: (e as? FgEligibility.Ineligible)?.reason ?: (e as? FgEligibility.Suspended)?.reason,
      experimental = when (e) {
        is FgEligibility.Eligible -> e.experimental
        is FgEligibility.Suspended -> isExperimental(lastVulkanLevel, deviceInfo)
        else -> false
      },
      profile = prefs.profile.get(),
      stats = if (active || failure != null) lastStats else null,
      noInputTimeout = noInputTimeout && failure != null,
      audioCompMs = appliedCompSec * MS_PER_S,
      refreshHz = capabilities.currentHz(),
      overloaded = active && overload.overloaded,
      measuredInFps = overload.inFps,
      sourceFps = sourceFps,
      liveFps = if (active) lastLive else null,
      bound = port != null,
      eligibilityKnown = libLoaded != null && shadersReady != null,
      shadersImported = if (libLoaded == false) false else shadersReady,
      shadersImportedAt = shaderManifest?.importedAt,
      settings = store.settings.value,
    )
  }

  companion object {
    private const val TAG = "FgController"

    /** Stats poll period while active. */
    const val POLL_INTERVAL_MS = 500L

    private const val NS_PER_MS = 1_000_000.0
    private const val NS_PER_S = 1_000_000_000.0
    private const val MS_PER_S = 1000.0

    /** E14: a logcat stats line every this many readings (with the stats switch on). */
    private const val STATS_LOG_EVERY_READINGS = 2

    /** REV 10: consecutive START_FAILED on one file that block it (I9). */
    const val MAX_CONSECUTIVE_START_FAILURES = 2

    /** REV 5: a native start longer than this is logged. */
    private const val START_LOG_THRESHOLD_MS = 100L

    /** REV 16: `seeking=false` and PLAYBACK_RESTART of one seek within this window flush once. */
    private const val SEEK_END_FLUSH_DEBOUNCE_MS = 250L

    /** FIX-3 (m1): longest wait for container-fps / video geometry before the first start of a file. */
    const val FPS_WAIT_MS = 1_000L

    /** FIX-3 (watchdog R1): unpaused time without input frames after a start that is a failure. */
    const val WATCHDOG_NO_INPUT_MS = 2_000L

    /** FIX-5 (§ 13.1.5): system property that replaces the profile input scale (measurements). */
    private const val DEBUG_INPUT_SCALE_PROPERTY = "debug.tatakai.fg.input_scale"
    private const val DEBUG_INPUT_SCALE_MIN = 0.25f
    private const val DEBUG_INPUT_SCALE_MAX = 1.0f

    /** Tatakai: 8K and above (either orientation) suspends FG (Enigmata's EIGHT_K reason). */
    private const val EIGHT_K_LONG_SIDE = 7680
    private const val EIGHT_K_SHORT_SIDE = 4320

    /** Tatakai: the video display size is 8K (pure). */
    internal fun isEightK(video: Pair<Int, Int>?): Boolean {
      video ?: return false
      return max(video.first, video.second) >= EIGHT_K_LONG_SIDE || min(video.first, video.second) >= EIGHT_K_SHORT_SIDE
    }

    /** `android.os.SystemProperties.get(key)` by reflection (hidden API); null when it cannot be read (e.g. on the JVM). */
    private fun readSystemProperty(key: String): String? =
      try {
        Class.forName("android.os.SystemProperties").getMethod("get", String::class.java).invoke(null, key) as? String
      } catch (e: ReflectiveOperationException) {
        null
      } catch (e: RuntimeException) {
        null
      }

    /** REV 7: base margin added to the source period for the initial L (engine compute + present). */
    const val INITIAL_LATENCY_MARGIN_MS = 8.0

    /** REV 7: initial L for a source period [periodNs] (0 = unknown -> margin only), quantized. */
    fun initialLatencyMs(periodNs: Long): Double =
      quantizeLatency((if (periodNs > 0) periodNs / NS_PER_MS else 0.0) + INITIAL_LATENCY_MARGIN_MS)

    /** REV 8: the «experimental» rule of [FgPolicy.eligibility] for states that are not Eligible (a suspension). */
    fun isExperimental(level: Int, info: FgDeviceInfo?): Boolean =
      (
        FgPolicy.eligibility(
          abiOk = true, libLoaded = true, vulkanLevel = level, allowUntested = true, gpuName = info?.gpuName,
          shadersReady = true, inPip = false, background = false, lowPower = false, eightK = false,
          hdrPassthrough = false,
        ) as? FgEligibility.Eligible
        )?.experimental ?: false

    fun sourcePeriodNs(fps: Double?): Long =
      if (fps == null || !fps.isFinite() || fps <= 0.0) 0L else (1_000_000_000.0 / fps).toLong()

    /** Same quantization as [FgPolicy.shouldUpdateLatency] ([FgPolicy.LATENCY_QUANTUM_MS]). */
    fun quantizeLatency(ms: Double): Double =
      if (!ms.isFinite() || ms <= 0.0) 0.0 else Math.round(ms / FgPolicy.LATENCY_QUANTUM_MS) * FgPolicy.LATENCY_QUANTUM_MS
  }
}
