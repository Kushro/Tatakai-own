/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import android.view.Surface
import java.io.File

/**
 * R13 frame generation: the only Kotlin entry to `libtatakai_fg.so` (`cpp/fg/fg_jni.cpp`).
 *
 * The library is loaded once inside `runCatching`; it only exists for arm64-v8a and x86_64, so on
 * other ABIs (or on any load failure) [isAvailable] is false and every method returns its neutral
 * value without touching native code (invariant I1). Every call is also wrapped so that any
 * `Throwable` (for example `UnsatisfiedLinkError`) becomes an [FgResult] code or a neutral value;
 * nothing thrown here reaches the player.
 *
 * Contract with `fg_jni.cpp` (its header table is the source of truth):
 * 1. The natives are registered in `JNI_OnLoad` with `RegisterNatives` from one table on
 *    `app/tatakai/next/framegen/FgNative`. They are `private external fun` members of this `object`
 *    (instance natives, no `@JvmStatic`), so the C entries receive the object instance. The
 *    proguard rule keeps the class name and the native member names. If registration fails,
 *    `loadLibrary` throws and [isAvailable] is false.
 * 2. No C++ exception crosses JNI: failures come back as an [FgResult] code, null, false or a
 *    no-op. The only pending Java exception is an `OutOfMemoryError` from `nativeStats` when the
 *    array itself cannot be allocated; [stats] catches it.
 * 3. [createInputSurface] opens the AImageReader input (replacing one opened but never started)
 *    and returns its window as a `Surface`; null if a size is <= 0, a pipeline is running or the
 *    reader fails. The Surface is invalid after [stop]; the caller must call `Surface.release()`.
 * 4. [start] needs an open input (else NOT_INITIALIZED), returns BUSY while running, INVALID_ARG on
 *    bad values or when the input size differs from [createInputSurface]'s, SHADERS_MISSING when
 *    `shaderDir` lacks the FP32 set. On failure everything it created is undone and the input stays
 *    open (a retry may reuse it; [stop] releases it). Internal exception: CONTEXT.
 * 5. [updateConfig] is queued (finalize + initialize on the engine thread): OK, NOT_INITIALIZED
 *    without a running pipeline, INVALID_ARG when the input or output size changes (that needs a
 *    stop/start cycle). [setSourcePeriodNs], [flush] and [setPaused] are no-ops without a running
 *    pipeline; a negative period is passed as 0.
 * 6. [stop] is blocking and idempotent (I3 order) and also releases an input that was opened but
 *    never started. [stats] always has [FgStats.SIZE] entries; after [stop] it is the last snapshot
 *    with RUNNING/PAUSED reported as STOPPED (FAILED kept, I9); never started: zeros.
 *    [deviceInfo] is null on error and also while a pipeline runs (the probe would touch volk
 *    globals owned by framegen).
 */
object FgNative {
  private const val LIBRARY = "tatakai_fg"

  private val loaded: Boolean = runCatching { System.loadLibrary(LIBRARY) }.isSuccess

  /** Whether `libtatakai_fg.so` loaded and registered its natives. */
  fun isAvailable(): Boolean = loaded

  /** GPU capabilities, or null when unavailable, on error or while a pipeline runs. */
  fun deviceInfo(): FgDeviceInfo? =
    guarded(null, null) { FgDeviceInfo.parse(nativeDeviceInfo()) }

  /**
   * Extracts the SPIR-V shaders of [dll] into [outDir]. Returns the number of FP32 shaders written
   * (>= 0) or a negative [FgResult] code: DLL_UNREADABLE, MISSING_SPIRV, BINDING_REWRITE,
   * WRITE_FAILED, INVALID_ARG; NOT_INITIALIZED when the library is unavailable.
   */
  fun extractShaders(dll: File, outDir: File): Int =
    guarded(FgResult.NOT_INITIALIZED.code, FgResult.DLL_UNREADABLE.code) {
      nativeExtractShaders(dll.absolutePath, outDir.absolutePath)
    }

  /** True when `dir/fp32` holds the complete FP32 set. */
  fun probeShaders(dir: File): Boolean =
    guarded(false, false) { nativeProbeShaders(dir.absolutePath) }

  /** Opens the input AImageReader of [width] x [height]; see note 3 of the class KDoc. */
  fun createInputSurface(width: Int, height: Int): Surface? =
    guarded(null, null) { nativeCreateInputSurface(width, height) }

  /** Starts the pipeline presenting to [outSurface]; returns an [FgResult] code (note 4). */
  fun start(outSurface: Surface, cfg: FgConfig): Int =
    guarded(FgResult.NOT_INITIALIZED.code, FgResult.CONTEXT.code) {
      nativeStart(outSurface, cfg.packInts(), cfg.packFloats(), cfg.shaderDir)
    }

  /** Queues a new config for the running pipeline; returns an [FgResult] code (note 5). */
  fun updateConfig(cfg: FgConfig): Int =
    guarded(FgResult.NOT_INITIALIZED.code, FgResult.CONTEXT.code) {
      nativeUpdateConfig(cfg.packInts(), cfg.packFloats(), cfg.shaderDir)
    }

  /** Source frame period estimate in ns (no-op without a running pipeline). */
  fun setSourcePeriodNs(ns: Long) {
    guarded(Unit, Unit) { nativeSetSourcePeriodNs(ns) }
  }

  /** Seek or discontinuity: drops queued frames and reseeds (I4). */
  fun flush() {
    guarded(Unit, Unit) { nativeFlush() }
  }

  /** Pause keeps the last real frame and generates nothing (I5). */
  fun setPaused(paused: Boolean) {
    guarded(Unit, Unit) { nativeSetPaused(paused) }
  }

  /** Blocking, idempotent teardown (I3). */
  fun stop() {
    guarded(Unit, Unit) { nativeStop() }
  }

  /** Raw stats with the [FgStats] indices; always [FgStats.SIZE] entries (zeros when unavailable). */
  fun stats(): LongArray =
    guarded(LongArray(FgStats.SIZE), LongArray(FgStats.SIZE)) {
      val raw: LongArray? = nativeStats()
      raw?.copyOf(FgStats.SIZE) ?: LongArray(FgStats.SIZE)
    }

  /** Runs [block] only when loaded; [unavailable] when not, [failure] on any Throwable. */
  private inline fun <T> guarded(unavailable: T, failure: T, block: () -> T): T {
    if (!loaded) return unavailable
    return try {
      block()
    } catch (t: Throwable) {
      failure
    }
  }

  // Native table of fg_jni.cpp (kMethods). Names and signatures must match it 1:1.
  private external fun nativeDeviceInfo(): String?
  private external fun nativeExtractShaders(dllPath: String, outDir: String): Int
  private external fun nativeProbeShaders(dir: String): Boolean
  private external fun nativeCreateInputSurface(width: Int, height: Int): Surface?
  private external fun nativeStart(outSurface: Surface, ints: IntArray, floats: FloatArray, shaderDir: String): Int
  private external fun nativeUpdateConfig(ints: IntArray, floats: FloatArray, shaderDir: String): Int
  private external fun nativeSetSourcePeriodNs(ns: Long)
  private external fun nativeFlush()
  private external fun nativeSetPaused(paused: Boolean)
  private external fun nativeStop()
  private external fun nativeStats(): LongArray
}
