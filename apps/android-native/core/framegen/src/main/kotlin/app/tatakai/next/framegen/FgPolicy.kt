/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import kotlin.math.abs
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt
import kotlin.math.sqrt

/**
 * R13: result of [FgPolicy.eligibility] (facts 13, 16 and 22).
 *
 * - [Eligible]: FG can run; [Eligible.experimental] shows the «experimental» notice (fact 13).
 * - [Suspended]: paused by the current situation, resumes by itself (fact 16).
 * - [Ineligible]: the device or install cannot run FG (facts 13 and 22).
 */
sealed interface FgEligibility {
  data class Eligible(val experimental: Boolean) : FgEligibility

  data class Suspended(val reason: FgUnavailableReason) : FgEligibility

  data class Ineligible(val reason: FgUnavailableReason) : FgEligibility
}

/**
 * R13: the Auto multiplier choice (fact 4).
 *
 * @property n the multiplier, always in [FgPolicy.MIN_MULTIPLIER]..[FgPolicy.MAX_MULTIPLIER].
 * @property targetHz the display refresh the output aims at (a supported mode when it matches,
 *   otherwise the current refresh).
 * @property exact fps×[n] matches [targetHz] within [FgPolicy.MATCH_TOLERANCE].
 */
data class AutoChoice(val n: Int, val targetHz: Float, val exact: Boolean)

/**
 * Amendment 3 (E18, D1b): state of the Auto budget rule ([FgPolicy.autoBudgetStep]).
 *
 * @property startedMs when FG started running with the current file/multiplier.
 * @property lastStepMs when the last step down happened, or null if none yet.
 * @property windowStartMs start of the current observation window, or null when there is none.
 * @property windowOutFrames output frame counter at [windowStartMs].
 * @property windowDropped dropped-late counter at [windowStartMs].
 */
data class FgBudgetState(
  val startedMs: Long,
  val lastStepMs: Long? = null,
  val windowStartMs: Long? = null,
  val windowOutFrames: Long = 0L,
  val windowDropped: Long = 0L,
)

/**
 * Amendment 3 (E18): one reading for [FgPolicy.autoBudgetStep].
 *
 * @property nowMs monotonic time of the reading.
 * @property auto the multiplier is in Auto mode.
 * @property n the multiplier in effect.
 * @property counting the counters advance (no pause, cache wait or seek).
 * @property outFrames cumulative output frames.
 * @property droppedLate cumulative frames dropped for being late.
 * @property computeNsAvg average compute time of an input pair, in ns.
 * @property sourcePeriodNs nominal frame period of the source, in ns (≤ 0 = unknown).
 */
data class FgBudgetSample(
  val nowMs: Long,
  val auto: Boolean,
  val n: Int,
  val counting: Boolean,
  val outFrames: Long,
  val droppedLate: Long,
  val computeNsAvg: Long,
  val sourcePeriodNs: Long,
)

/**
 * Amendment 3 (E18): result of [FgPolicy.autoBudgetStep].
 *
 * @property state the state for the next reading.
 * @property stepTo the lower multiplier to apply now, or null to keep the current one.
 */
data class FgBudgetResult(val state: FgBudgetState, val stepTo: Int?)

/**
 * Amendment 3 (E19, D1c): state of the overload detector ([FgPolicy.overloadStep]).
 *
 * @property belowSinceMs since when the smoothed input stays below the threshold, or null.
 * @property overloaded the input stayed below the threshold for more than [FgPolicy.OVERLOAD_MIN_MS].
 * @property inFps the smoothed input fps (0 = no reading yet).
 */
data class FgOverloadState(val belowSinceMs: Long? = null, val overloaded: Boolean = false, val inFps: Double = 0.0)

/**
 * FIX-5 (§ 13.1.2): what FG gives up when the quality engine and the GPU budget compete.
 *
 * - [FLUIDITY] (default): keeps the chosen or Auto multiplier and lowers the intermediate input size.
 * - [SHARPNESS]: caps the multiplier at 2× and uses the largest intermediate size that fits.
 *
 * Tatakai: the enum lives in :core:model (`FrameGenPriority`, persisted in `AppSettings.frameGen.priority`).
 */
typealias FgPriority = app.tatakai.next.model.FrameGenPriority

/**
 * FIX-5 (§ 13.1.6): GPU cost class of [FgPolicy.budgetInputScale] (see [FgPolicy.gpuClassOf]).
 *
 * - [A7XX_PLUS]: Qualcomm Adreno 7xx/8xx, coefficients measured on the Tab S9+ (Adreno 740).
 * - [OTHER]: any other GPU, provisional cost factor [FgPolicy.OTHER_GPU_COST_FACTOR].
 */
enum class FgGpuClass { A7XX_PLUS, OTHER }

/**
 * FIX-5: result of [FgPolicy.inputSizeHybrid].
 *
 * @property width AImageReader input width (even, ≥ [FgPolicy.MIN_INPUT_DIMENSION]).
 * @property height AImageReader input height (even, ≥ [FgPolicy.MIN_INPUT_DIMENSION]).
 * @property effectiveN the multiplier to run: the requested one clamped to 2..5, and at most 2 with
 *   [FgPriority.SHARPNESS] while the quality engine drives the size.
 */
data class FgInputPlan(val width: Int, val height: Int, val effectiveN: Int)

/** R13: pure frame generation policy. No Android dependencies; fully unit-tested (W4b). */
object FgPolicy {
  /** Smallest multiplier (fact 4: «mínimo 2×»). */
  const val MIN_MULTIPLIER = 2

  /** Largest multiplier (fact 4: «entre 2 y 5»). */
  const val MAX_MULTIPLIER = 5

  /** Relative tolerance for fps×N to match a refresh rate (fact 4: ±0,5 %). */
  const val MATCH_TOLERANCE = 0.005

  /** Adreno model from which a GPU is in the tested range (fact 13: «Adreno 7xx o superior»). */
  const val TESTED_ADRENO_MIN_MODEL = 700

  /** Smallest input dimension returned by [inputSize]. */
  const val MIN_INPUT_DIMENSION = 2

  /** Latency is quantized to this step before comparing (avoids audio-delay churn). */
  const val LATENCY_QUANTUM_MS = 4.0

  /** Minimum change of the quantized latency that justifies a new audio-delay. */
  const val LATENCY_MIN_DELTA_MS = 8.0

  /** Minimum time between two audio-delay updates caused by latency. */
  const val LATENCY_MIN_INTERVAL_MS = 2000L

  /**
   * Vulkan levels, encoded like `VK_MAKE_API_VERSION(0, major, minor, 0)` and like the version
   * argument of `PackageManager.FEATURE_VULKAN_HARDWARE_VERSION`. A level with a patch number (for
   * example 1.1.128 = 0x00401080) compares correctly against them. 0 means «no Vulkan / unknown».
   */
  const val VULKAN_1_1 = 0x00401000

  /** Vulkan 1.2; see [VULKAN_1_1]. */
  const val VULKAN_1_2 = 0x00402000

  /** Vulkan 1.3, the tested minimum (fact 13); see [VULKAN_1_1]. */
  const val VULKAN_1_3 = 0x00403000

  /**
   * Amendment 3 (E16b), FIX-5 (§ 13.1.4): default input limit floor. 1.0 = the input is never smaller
   * than the video on its long side, on every GPU (it was 0, «Estricto», before FIX-5).
   */
  const val VIDEO_FIT_FLOOR_DEFAULT = 1.0

  /**
   * Amendment 3 (E16b): allowed values of the input limit floor (strict, 50 %, 75 %); FIX-5 adds 1.0
   * (never smaller than the video), the new default.
   */
  val VIDEO_FIT_FLOORS: List<Float> = listOf(0f, 0.5f, 0.75f, 1f)

  /**
   * FIX-5 (§ 13.1.1): the GPU budget aims at LSFG compute ≤ this fraction of the source frame period
   * (`reports/fg-vs-upscale.md`, «Regla práctica: LSFG ≤ 50 % del periodo»).
   */
  const val BUDGET_PERIOD_FRACTION = 0.5

  /** FIX-5: frame rate assumed by [budgetInputScale] when the source fps is unknown or invalid. */
  const val BUDGET_DEFAULT_FPS = 30.0

  /**
   * FIX-5, PROVISIONAL: fixed LSFG cost per source frame, in ms (c0 of comp(N) ≈ c0 + MP·(cFlow +
   * cGen·(N−1))). Measured on the Tab S9+ (Adreno 740, LSFG 3.1 performance mode) from logcat at
   * 450×720, 1150×720 and 1726×1080; `reports/fg-vs-upscale.md` § «Modelo de coste».
   */
  const val COST_BASE_MS = 3.0

  /** FIX-5, PROVISIONAL: optical flow cost per input megapixel, in ms (cFlow); see [COST_BASE_MS]. */
  const val COST_FLOW_MS_PER_MP = 2.3

  /** FIX-5, PROVISIONAL: cost of each generated frame per input megapixel, in ms (cGen); see [COST_BASE_MS]. */
  const val COST_GEN_MS_PER_MP = 2.4

  /**
   * FIX-5, PROVISIONAL and not measured: factor applied to cFlow and cGen on [FgGpuClass.OTHER]
   * (conservative, so the intermediate size bounds itself on weaker GPUs such as the Adreno 619).
   */
  const val OTHER_GPU_COST_FACTOR = 4.0

  /**
   * FIX-5, PROVISIONAL and not measured: factor applied to cFlow and cGen with the LSFG quality mode
   * (`performanceMode = false`); the coefficients above were measured in performance mode.
   */
  const val QUALITY_MODE_COST_FACTOR = 2.0

  /**
   * FIX-5 (§ 13.1.1): with the quality engine on, the input never exceeds this multiple of the video
   * (covering it on its long side): Anime4K/Fidelity upscale passes need 1.2–2.0×.
   */
  const val HYBRID_MAX_VIDEO_FACTOR = 2.0

  /** FIX-5 (§ 13.1.2): largest multiplier with [FgPriority.SHARPNESS]. */
  const val SHARPNESS_MAX_MULTIPLIER = 2

  /** FIX-5: Adreno models of [FgGpuClass.A7XX_PLUS] (7xx and 8xx). */
  private val A7XX_PLUS_MODELS = 700..899

  /** Amendment 3 (E16): the input never goes below this short side (unless the output is smaller). */
  const val MIN_INPUT_SHORT_SIDE = 360

  /** Amendment 3 (E18): largest Auto multiplier on an experimental GPU. */
  const val AUTO_EXPERIMENTAL_MAX_MULTIPLIER = 3

  /** Amendment 3 (E18): Auto does not step down before FG has run this long. */
  const val BUDGET_MIN_RUNNING_MS = 3_000L

  /** Amendment 3 (E18): minimum time between two Auto step-downs. */
  const val BUDGET_MIN_STEP_INTERVAL_MS = 5_000L

  /** Amendment 3 (E18): length of the observation window. */
  const val BUDGET_WINDOW_MS = 2_000L

  /** Amendment 3 (E18): compute over budget when its average exceeds this fraction of the source period. */
  const val BUDGET_COMPUTE_FRACTION = 0.85

  /** Amendment 3 (E18): drops over budget when they exceed this fraction of the window output. */
  const val BUDGET_DROP_FRACTION = 0.10

  /** Amendment 3 (E19): overload when the input falls below this fraction of the source fps. */
  const val OVERLOAD_INPUT_FRACTION = 0.80

  /** Amendment 3 (E19): the input must stay low for more than this to be an overload. */
  const val OVERLOAD_MIN_MS = 5_000L

  private val ADRENO_MODEL = Regex("adreno\\D*(\\d+)", RegexOption.IGNORE_CASE)

  private val API_VERSION_TEXT = Regex("^\\s*(\\d+)\\.(\\d+)(?:\\.(\\d+))?")

  /**
   * Eligibility of FG right now (facts 13, 16 and 22; Amendment 1, point 1).
   *
   * Ineligible reasons win over suspensions and are checked in this order: [FgUnavailableReason.ABI],
   * [FgUnavailableReason.LIBRARY], [FgUnavailableReason.VULKAN], [FgUnavailableReason.SHADERS].
   * Then the suspensions, in order: Tatakai Link's [FgUnavailableReason.REMOTE] ([remote], first so the
   * chip says «PC»), then those of fact 16: [FgUnavailableReason.PIP],
   * [FgUnavailableReason.BACKGROUND], [FgUnavailableReason.LOW_POWER], [FgUnavailableReason.EIGHT_K],
   * [FgUnavailableReason.HDR_PASSTHROUGH]. Otherwise [FgEligibility.Eligible].
   *
   * Vulkan ([vulkanLevel], see [VULKAN_1_1]; the caller folds missing AHB/swapchain into 0):
   * - below [VULKAN_1_1] → [FgUnavailableReason.VULKAN] always;
   * - below [VULKAN_1_3] → [FgUnavailableReason.VULKAN] unless [allowUntested]
   *   (`FgPreferences.allowUntestedVulkan`, «Allow FG on untested GPUs (experimental)»).
   *
   * [FgEligibility.Eligible.experimental] is true when [vulkanLevel] < [VULKAN_1_3], or when
   * [gpuName] is known and [isExperimentalGpu]. With [gpuName] null (device not probed yet) the GPU
   * alone does not mark it experimental (REV 8).
   *
   * I10: with [allowUntested] = false the result equals the pre-amendment rule with
   * `vk13 = vulkanLevel >= VULKAN_1_3` for every input, with one deliberate difference (REV 8):
   * `gpuName == null` with level ≥ 1.3 now gives `Eligible(experimental = false)` instead of
   * `Eligible(experimental = true)`.
   */
  fun eligibility(
    abiOk: Boolean,
    libLoaded: Boolean,
    vulkanLevel: Int,
    allowUntested: Boolean,
    gpuName: String?,
    shadersReady: Boolean,
    inPip: Boolean,
    background: Boolean,
    lowPower: Boolean,
    eightK: Boolean,
    hdrPassthrough: Boolean,
    remote: Boolean = false,
  ): FgEligibility {
    val vulkanOk = vulkanLevel >= VULKAN_1_3 || (allowUntested && vulkanLevel >= VULKAN_1_1)
    return when {
      !abiOk -> FgEligibility.Ineligible(FgUnavailableReason.ABI)
      !libLoaded -> FgEligibility.Ineligible(FgUnavailableReason.LIBRARY)
      !vulkanOk -> FgEligibility.Ineligible(FgUnavailableReason.VULKAN)
      !shadersReady -> FgEligibility.Ineligible(FgUnavailableReason.SHADERS)
      remote -> FgEligibility.Suspended(FgUnavailableReason.REMOTE)
      inPip -> FgEligibility.Suspended(FgUnavailableReason.PIP)
      background -> FgEligibility.Suspended(FgUnavailableReason.BACKGROUND)
      lowPower -> FgEligibility.Suspended(FgUnavailableReason.LOW_POWER)
      eightK -> FgEligibility.Suspended(FgUnavailableReason.EIGHT_K)
      hdrPassthrough -> FgEligibility.Suspended(FgUnavailableReason.HDR_PASSTHROUGH)
      else -> FgEligibility.Eligible(
        experimental = (gpuName != null && isExperimentalGpu(gpuName)) || vulkanLevel < VULKAN_1_3,
      )
    }
  }

  /**
   * Amendment 1: the Vulkan level to judge eligibility with. When the native probe ran
   * ([probedApiVersion] = `FgDeviceInfo.apiVersion`, text `major.minor.patch`), its value has
   * priority over [declaredLevel] (the `FEATURE_VULKAN_HARDWARE_VERSION` level); null or
   * unparseable text → [declaredLevel].
   */
  fun effectiveVulkanLevel(declaredLevel: Int, probedApiVersion: String?): Int =
    parseApiVersion(probedApiVersion) ?: declaredLevel

  /**
   * `major.minor[.patch]` → `VK_MAKE_API_VERSION(0, major, minor, patch)`, or null when the text
   * does not start with that pattern or a part is out of range (major 0..127, minor 0..1023,
   * patch 0..4095).
   */
  fun parseApiVersion(text: String?): Int? {
    val m = API_VERSION_TEXT.find(text ?: return null) ?: return null
    val major = m.groupValues[1].toIntOrNull() ?: return null
    val minor = m.groupValues[2].toIntOrNull() ?: return null
    val patchText = m.groupValues[3]
    val patch = if (patchText.isEmpty()) 0 else patchText.toIntOrNull() ?: return null
    if (major !in 0..127 || minor !in 0..1023 || patch !in 0..4095) return null
    return (major shl 22) or (minor shl 12) or patch
  }

  /**
   * Fact 13: the GPU is outside the tested range. False only when [gpuName] contains «Adreno»
   * (case-insensitive) followed by a model number ≥ [TESTED_ADRENO_MIN_MODEL] (for example
   * «Adreno (TM) 740»). Unknown names, null, Mali, PowerVR, older Adreno → true.
   */
  fun isExperimentalGpu(gpuName: String?): Boolean {
    if (gpuName == null) return true
    val model = ADRENO_MODEL.find(gpuName)?.groupValues?.get(1)?.toIntOrNull() ?: return true
    return model < TESTED_ADRENO_MIN_MODEL
  }

  /**
   * FIX-5 (§ 13.1.6): cost class of [gpuName] (`FgDeviceInfo.gpuName`). [FgGpuClass.A7XX_PLUS] when it
   * contains «Adreno» (case-insensitive) followed by a model in 700..899 (for example
   * «Adreno (TM) 740»); null, unknown names, Mali, PowerVR, older or newer Adreno → [FgGpuClass.OTHER].
   */
  fun gpuClassOf(gpuName: String?): FgGpuClass {
    val model = ADRENO_MODEL.find(gpuName ?: return FgGpuClass.OTHER)?.groupValues?.get(1)?.toIntOrNull()
    return if (model != null && model in A7XX_PLUS_MODELS) FgGpuClass.A7XX_PLUS else FgGpuClass.OTHER
  }

  /**
   * Fact 4: the Auto multiplier.
   *
   * 1. With [allowModeSwitch] (default): the smallest N in 2..5 such that fps×N matches (±0,5 %)
   *    any display mode in [supportedHz] or [currentHz]; target = that mode (closest one if several
   *    match the same N).
   * 2. Otherwise, or when step 1 finds nothing: the smallest N such that fps×N matches [currentHz]
   *    (±0,5 %). «Divisor exacto» of the current refresh is read as fps×N == currentHz within the
   *    tolerance (24 fps @ 120 Hz → 5×, 30 fps @ 60 Hz → 2×).
   * 3. No match: the largest N with fps×N ≤ [currentHz] («lo más cerca posible sin superarlo»),
   *    at least 2×; target = [currentHz], exact = false.
   *
   * Invalid [fps] (NaN, infinite or ≤ 0) → N = 2, target [currentHz], exact = false. Invalid
   * entries of [supportedHz] (NaN, infinite or ≤ 0) are ignored.
   *
   * Amendment 3 (E18): [maxN] is clamped to [MIN_MULTIPLIER]..[MAX_MULTIPLIER] and every step only
   * tries N in [MIN_MULTIPLIER]..maxN (for example [AUTO_EXPERIMENTAL_MAX_MULTIPLIER] on an
   * experimental GPU, or the cap left by [autoBudgetStep]). The default keeps the previous behaviour.
   */
  fun autoMultiplier(
    fps: Double,
    currentHz: Float,
    supportedHz: List<Float>,
    allowModeSwitch: Boolean,
    maxN: Int = MAX_MULTIPLIER,
  ): AutoChoice {
    if (!fps.isFinite() || fps <= 0.0) return AutoChoice(MIN_MULTIPLIER, currentHz, exact = false)
    val currentValid = isValidHz(currentHz)
    val top = maxN.coerceIn(MIN_MULTIPLIER, MAX_MULTIPLIER)

    if (allowModeSwitch) {
      val modes = (supportedHz + currentHz).filter(::isValidHz).distinct()
      for (n in MIN_MULTIPLIER..top) {
        val out = fps * n
        val best = modes.filter { matches(out, it) }.minByOrNull { abs(out - it) }
        if (best != null) return AutoChoice(n, best, exact = true)
      }
    }

    if (currentValid) {
      for (n in MIN_MULTIPLIER..top) {
        if (matches(fps * n, currentHz)) return AutoChoice(n, currentHz, exact = true)
      }
    }

    var n = MIN_MULTIPLIER
    if (currentValid) {
      for (candidate in MIN_MULTIPLIER..top) {
        if (fps * candidate <= currentHz) n = candidate
      }
    }
    return AutoChoice(n, currentHz, exact = false)
  }

  /**
   * Fact 25: the tuning in effect for [p]. Presets return their fixed values; [FgProfile.CUSTOM]
   * returns [custom] bounded to the valid ranges: flowScale clamped to
   * [FgTuning.FLOW_SCALE_MIN]..[FgTuning.FLOW_SCALE_MAX] (NaN → the BALANCED flow scale) and
   * inputScale snapped to the nearest of [FgTuning.INPUT_SCALES] (NaN → 1.0).
   */
  fun resolveProfile(p: FgProfile, custom: FgTuning): FgTuning {
    p.presetTuning()?.let { return it }
    val flow =
      if (custom.flowScale.isNaN()) {
        FgTuning.BALANCED.flowScale
      } else {
        custom.flowScale.coerceIn(FgTuning.FLOW_SCALE_MIN, FgTuning.FLOW_SCALE_MAX)
      }
    val input =
      if (custom.inputScale.isNaN()) {
        FgTuning.BALANCED.inputScale
      } else {
        FgTuning.INPUT_SCALES.minBy { abs(it - custom.inputScale) }
      }
    return FgTuning(performanceMode = custom.performanceMode, flowScale = flow, inputScale = input)
  }

  /**
   * Fact 4 (design: SurfaceFrameRate with CHANGE_FRAME_RATE_ALWAYS): the frame rate to request from the
   * Surface while FG runs. Null means «keep the current A5b behaviour»: [matchRefresh] is off,
   * [fps] is null/NaN/infinite/≤ 0, or [n] is outside 2..5. Otherwise fps×n.
   */
  fun targetFrameRate(fps: Double?, n: Int, matchRefresh: Boolean): Double? {
    if (!matchRefresh || fps == null || !fps.isFinite() || fps <= 0.0) return null
    if (n !in MIN_MULTIPLIER..MAX_MULTIPLIER) return null
    return fps * n
  }

  /**
   * Fact 25: AImageReader input size for an output Surface of [w]×[h] and an input [scale]
   * (0.75 or 1.0). Each side is rounded to the nearest integer and then lowered to an even value
   * (never larger than the scaled size, so with scale ≤ 1 never larger than the Surface), with a
   * minimum of [MIN_INPUT_DIMENSION]. A non-finite or ≤ 0 [scale] is treated as 1.0.
   */
  fun inputSize(w: Int, h: Int, scale: Float): Pair<Int, Int> {
    val s = if (scale.isFinite() && scale > 0f) scale else 1f
    return evenDimension(w, s.toDouble()) to evenDimension(h, s.toDouble())
  }

  /**
   * Amendment 3 (E16, E16b; D1a): input size limited to the video. The input keeps the aspect ratio
   * of the [outW]×[outH] output within the even rounding (the presenter scales it up with a blit)
   * and, with scale ≤ 1, is never larger than the output.
   *
   * With s = [scale] (NaN, ≤ 0 or infinite → 1) and [video] = vw×vh (see [videoDisplaySize]):
   * - kIn = min(vw/outW, vh/outH), the largest factor that does not exceed the video on any side;
   * - kOut = min(1, max(vw/outW, vh/outH)), the factor that covers the video on its long side;
   * - kMin = min(1, [MIN_INPUT_SHORT_SIDE] / min(outW, outH));
   * - k = min(s, max(kIn, [floor]·kOut, kMin)).
   *
   * Each side is outW·k, outH·k rounded and lowered to even like [inputSize] (minimum
   * [MIN_INPUT_DIMENSION]). [video] null (or with a side ≤ 0) or an output with a side ≤ 0 →
   * `inputSize(outW, outH, s)`. [floor] arrives already snapped by [snapVideoFitFloor]; a
   * non-finite value is treated as 0. FIX-5: the default floor is 1.0 ([VIDEO_FIT_FLOOR_DEFAULT]).
   *
   * Examples, output 2460×1080, floor 0: 1920×1080 → 1920×842; 1280×720 → 1280×562; portrait
   * 1080×1920 → 1080×474 (1844×810 with floor 0.75). With floor 1.0 the input covers the video:
   * 1280×720 → 1640×720; 1920×1080 → 2460×1080.
   */
  fun inputSize(
    outW: Int,
    outH: Int,
    scale: Float,
    video: Pair<Int, Int>?,
    floor: Double = VIDEO_FIT_FLOOR_DEFAULT,
  ): Pair<Int, Int> {
    val s = if (scale.isFinite() && scale > 0f) scale else 1f
    if (video == null || outW <= 0 || outH <= 0 || video.first <= 0 || video.second <= 0) {
      return inputSize(outW, outH, s)
    }
    val rw = video.first.toDouble() / outW
    val rh = video.second.toDouble() / outH
    val kIn = min(rw, rh)
    val kOut = min(1.0, max(rw, rh))
    val kMin = min(1.0, MIN_INPUT_SHORT_SIDE.toDouble() / min(outW, outH))
    val f = if (floor.isFinite()) floor else 0.0
    val k = min(s.toDouble(), maxOf(kIn, f * kOut, kMin))
    return evenDimension(outW, k) to evenDimension(outH, k)
  }

  /**
   * FIX-5 (§ 13.1.1, `reports/fg-vs-upscale.md` § 1): the largest input factor k (of the [outW]×[outH]
   * output) whose LSFG compute fits the GPU budget:
   *
   * comp(N) ≈ [COST_BASE_MS] + MP·(cFlow + cGen·(N−1)) ≤ [BUDGET_PERIOD_FRACTION] × 1000 / fps,
   * with MP = outW·outH·k² / 10⁶.
   *
   * - cFlow = [COST_FLOW_MS_PER_MP] and cGen = [COST_GEN_MS_PER_MP] (provisional, measured on
   *   [FgGpuClass.A7XX_PLUS] in performance mode), both × [OTHER_GPU_COST_FACTOR] on
   *   [FgGpuClass.OTHER] and × [QUALITY_MODE_COST_FACTOR] when [performanceMode] is false.
   * - [fps] null, NaN, infinite or ≤ 0 → [BUDGET_DEFAULT_FPS]. [n] is clamped to
   *   [MIN_MULTIPLIER]..[MAX_MULTIPLIER].
   * - A budget at or below [COST_BASE_MS] → 0. An output with a side ≤ 0 → 1 (no budget limit).
   *
   * The result is not bounded by the scale or the video; [inputSizeHybrid] does that.
   * Example: 24 fps, N = 5, 2800×1752, A7XX_PLUS → k ≈ 0.553 (≈ 1548×968).
   */
  fun budgetInputScale(
    fps: Double?,
    n: Int,
    outW: Int,
    outH: Int,
    performanceMode: Boolean,
    gpuClass: FgGpuClass,
  ): Double {
    if (outW <= 0 || outH <= 0) return 1.0
    val rate = if (fps != null && fps.isFinite() && fps > 0.0) fps else BUDGET_DEFAULT_FPS
    val budgetMs = BUDGET_PERIOD_FRACTION * 1000.0 / rate
    if (budgetMs <= COST_BASE_MS) return 0.0
    val factor = (if (gpuClass == FgGpuClass.OTHER) OTHER_GPU_COST_FACTOR else 1.0) *
      (if (performanceMode) 1.0 else QUALITY_MODE_COST_FACTOR)
    val nn = n.coerceIn(MIN_MULTIPLIER, MAX_MULTIPLIER)
    val msPerMp = factor * (COST_FLOW_MS_PER_MP + COST_GEN_MS_PER_MP * (nn - 1))
    val maxMp = (budgetMs - COST_BASE_MS) / msPerMp
    val outMp = outW.toDouble() * outH / 1_000_000.0
    return sqrt(maxMp / outMp)
  }

  /**
   * FIX-5 (§ 13.1, design § 10.1): FG input size and effective multiplier for the hybrid policy. The
   * input keeps the aspect ratio of the [outW]×[outH] output (the presenter scales it up), its sides
   * are even and ≥ [MIN_INPUT_DIMENSION], and with scale ≤ 1 it is never larger than the output.
   *
   * With s = [scale] (NaN, ≤ 0 or infinite → 1), nReq = [n] clamped to 2..5, and for a valid
   * [video] = vw×vh: kIn = min(vw/outW, vh/outH), kCover = max(vw/outW, vh/outH) (covers the video on
   * its long side), kMin = min(1, [MIN_INPUT_SHORT_SIDE] / min(outW, outH)). In order:
   * 1. [screenResolution] («FG a resolución de pantalla») → k = s, effectiveN = nReq.
   * 2. [qualityActive] (Anime4K/Fidelity on) → effectiveN = min(nReq, [SHARPNESS_MAX_MULTIPLIER]) with
   *    [FgPriority.SHARPNESS], else nReq; kBudget = [budgetInputScale] with effectiveN;
   *    k = min(s, max(min([HYBRID_MAX_VIDEO_FACTOR]·kCover, kBudget), kIn, kMin)): the budget never
   *    takes the input below the video that fits. Without a valid video: k = min(s, max(kBudget, kMin)).
   * 3. Otherwise (engine off) → the size of `inputSize(outW, outH, scale, video, floor)` (default floor
   *    1.0: never smaller than the video), effectiveN = nReq.
   *
   * An output with a side ≤ 0 → `inputSize(outW, outH, s)` with the effectiveN of its branch.
   *
   * Example, 720p at 24 fps with N = 5 on a 2800×1752 A7XX_PLUS output: FLUIDITY → 1548×968, N 5;
   * SHARPNESS → 2462×1540, N 2; engine off → 1280×800; screen resolution → 2800×1752.
   */
  fun inputSizeHybrid(
    outW: Int,
    outH: Int,
    scale: Float,
    video: Pair<Int, Int>?,
    fps: Double?,
    n: Int,
    priority: FgPriority,
    gpuClass: FgGpuClass,
    screenResolution: Boolean,
    qualityActive: Boolean,
    floor: Double = VIDEO_FIT_FLOOR_DEFAULT,
    performanceMode: Boolean = true,
  ): FgInputPlan {
    val s = if (scale.isFinite() && scale > 0f) scale else 1f
    val nReq = n.coerceIn(MIN_MULTIPLIER, MAX_MULTIPLIER)
    val hybrid = qualityActive && !screenResolution
    val effectiveN =
      if (hybrid && priority == FgPriority.SHARPNESS) min(nReq, SHARPNESS_MAX_MULTIPLIER) else nReq
    val outputValid = outW > 0 && outH > 0
    val (w, h) = when {
      !outputValid || screenResolution -> inputSize(outW, outH, s)
      !hybrid -> inputSize(outW, outH, s, video, floor)
      else -> {
        val kBudget = budgetInputScale(fps, effectiveN, outW, outH, performanceMode, gpuClass)
        val kMin = min(1.0, MIN_INPUT_SHORT_SIDE.toDouble() / min(outW, outH))
        val k = if (video == null || video.first <= 0 || video.second <= 0) {
          min(s.toDouble(), max(kBudget, kMin))
        } else {
          val rw = video.first.toDouble() / outW
          val rh = video.second.toDouble() / outH
          val kTop = min(HYBRID_MAX_VIDEO_FACTOR * max(rw, rh), kBudget)
          min(s.toDouble(), maxOf(kTop, min(rw, rh), kMin))
        }
        evenDimension(outW, k) to evenDimension(outH, k)
      }
    }
    return FgInputPlan(w, h, effectiveN)
  }

  /**
   * Amendment 3 (E16): display size of the video from `video-params/w`, `video-params/h` and
   * `video-params/rotate`: [dw]×[dh], swapped when [rotate] % 180 == 90. Null when a side is
   * missing or ≤ 0.
   */
  fun videoDisplaySize(dw: Int?, dh: Int?, rotate: Int?): Pair<Int, Int>? {
    if (dw == null || dh == null || dw <= 0 || dh <= 0) return null
    return if (rotate != null && rotate % 180 == 90) dh to dw else dw to dh
  }

  /** Amendment 3 (E16b): [f] snapped to the nearest of [VIDEO_FIT_FLOORS] (NaN → 0). */
  fun snapVideoFitFloor(f: Float): Float {
    if (f.isNaN()) return 0f
    return VIDEO_FIT_FLOORS.minBy { abs(it - f) }
  }

  /**
   * Amendment 3 (E18, D1b): the Auto budget rule, evaluated on each statistics reading. Rules, in
   * order:
   * 1. Not [FgBudgetSample.auto], n ≤ [MIN_MULTIPLIER] or not counting → the window is cleared,
   *    no step.
   * 2. No window, or the output/dropped counters went down → a new window opens at (now, outFrames,
   *    droppedLate).
   * 3. The window is younger than [BUDGET_WINDOW_MS] → no change.
   * 4. computeOver = sourcePeriodNs > 0 and computeNsAvg > [BUDGET_COMPUTE_FRACTION] × sourcePeriodNs;
   *    dropOver = dropped in the window > [BUDGET_DROP_FRACTION] × output in the window (true too
   *    when no output and some drops).
   * 5. canStep = running for at least [BUDGET_MIN_RUNNING_MS] and at least
   *    [BUDGET_MIN_STEP_INTERVAL_MS] since the last step (or no step yet).
   * 6. Over budget and canStep → stepTo = n − 1, lastStepMs = now, the window is cleared.
   * 7. Otherwise a new window opens at now.
   */
  fun autoBudgetStep(state: FgBudgetState, sample: FgBudgetSample): FgBudgetResult {
    val now = sample.nowMs
    if (!sample.auto || sample.n <= MIN_MULTIPLIER || !sample.counting) {
      return FgBudgetResult(state.clearWindow(), stepTo = null)
    }
    val windowStart = state.windowStartMs
    if (windowStart == null ||
      sample.outFrames < state.windowOutFrames ||
      sample.droppedLate < state.windowDropped
    ) {
      return FgBudgetResult(state.openWindow(sample), stepTo = null)
    }
    if (now - windowStart < BUDGET_WINDOW_MS) return FgBudgetResult(state, stepTo = null)

    val dOut = sample.outFrames - state.windowOutFrames
    val dDrop = sample.droppedLate - state.windowDropped
    val computeOver =
      sample.sourcePeriodNs > 0 && sample.computeNsAvg > BUDGET_COMPUTE_FRACTION * sample.sourcePeriodNs
    val dropOver = dDrop > BUDGET_DROP_FRACTION * dOut
    val lastStep = state.lastStepMs
    val canStep = now - state.startedMs >= BUDGET_MIN_RUNNING_MS &&
      (lastStep == null || now - lastStep >= BUDGET_MIN_STEP_INTERVAL_MS)
    if ((computeOver || dropOver) && canStep) {
      return FgBudgetResult(state.clearWindow().copy(lastStepMs = now), stepTo = sample.n - 1)
    }
    return FgBudgetResult(state.openWindow(sample), stepTo = null)
  }

  /**
   * Amendment 3 (E19, D1c): the overload detector, evaluated on each statistics reading.
   *
   * - Not [counting] → a fresh [FgOverloadState].
   * - [inFps] null or not finite, or [sourceFps] null or ≤ 0 → [state] unchanged.
   * - The input is smoothed over two readings: sm = (state.inFps + inFps) / 2, or inFps when
   *   state.inFps is 0.
   * - sm < [OVERLOAD_INPUT_FRACTION] × sourceFps → below since `belowSinceMs ?: now`, overloaded
   *   when that lasted more than [OVERLOAD_MIN_MS]. Otherwise the detector restarts with inFps = sm.
   */
  fun overloadStep(
    state: FgOverloadState,
    nowMs: Long,
    inFps: Double?,
    sourceFps: Double?,
    counting: Boolean,
  ): FgOverloadState {
    if (!counting) return FgOverloadState()
    if (inFps == null || !inFps.isFinite() || sourceFps == null || sourceFps <= 0.0) return state
    val sm = if (state.inFps > 0.0) (state.inFps + inFps) / 2.0 else inFps
    if (sm < OVERLOAD_INPUT_FRACTION * sourceFps) {
      val since = state.belowSinceMs ?: nowMs
      return FgOverloadState(belowSinceMs = since, overloaded = nowMs - since > OVERLOAD_MIN_MS, inFps = sm)
    }
    return FgOverloadState(inFps = sm)
  }

  private fun FgBudgetState.clearWindow(): FgBudgetState =
    copy(windowStartMs = null, windowOutFrames = 0L, windowDropped = 0L)

  private fun FgBudgetState.openWindow(sample: FgBudgetSample): FgBudgetState =
    copy(windowStartMs = sample.nowMs, windowOutFrames = sample.outFrames, windowDropped = sample.droppedLate)

  /**
   * Fact 8: whether the audio-delay compensation must follow a new pipeline latency. [measuredMs] is
   * quantized to [LATENCY_QUANTUM_MS]; true when the quantized value differs from [appliedMs] by
   * at least [LATENCY_MIN_DELTA_MS] and at least [LATENCY_MIN_INTERVAL_MS] passed since the last
   * update. Non-finite inputs → false.
   */
  fun shouldUpdateLatency(appliedMs: Double, measuredMs: Double, msSinceLast: Long): Boolean {
    if (!appliedMs.isFinite() || !measuredMs.isFinite()) return false
    if (msSinceLast < LATENCY_MIN_INTERVAL_MS) return false
    return abs(quantizeLatency(measuredMs) - appliedMs) >= LATENCY_MIN_DELTA_MS
  }

  /** [measuredMs] rounded to the nearest multiple of [LATENCY_QUANTUM_MS]. */
  private fun quantizeLatency(measuredMs: Double): Double =
    Math.round(measuredMs / LATENCY_QUANTUM_MS) * LATENCY_QUANTUM_MS

  private fun isValidHz(hz: Float): Boolean = hz.isFinite() && hz > 0f

  private fun matches(out: Double, hz: Float): Boolean = abs(out - hz) <= MATCH_TOLERANCE * hz

  private fun evenDimension(size: Int, scale: Double): Int {
    val scaled = (size * scale).roundToInt()
    val even = scaled - (scaled and 1)
    return even.coerceAtLeast(MIN_INPUT_DIMENSION)
  }
}
