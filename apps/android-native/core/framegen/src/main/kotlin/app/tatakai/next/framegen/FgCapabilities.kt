/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import android.content.Context
import android.content.pm.PackageManager
import android.hardware.display.DisplayManager
import android.os.Build
import android.os.PowerManager
import android.view.Display

/**
 * R13: device facts [FgController] needs that are not FG state (Enigmata, top of `FgController.kt`). Injectable for JVM tests;
 * the production implementation is [AndroidFgCapabilities].
 *
 * The native Vulkan probe ([FgNative.deviceInfo]) is not here: the controller runs it itself, only when FG is about to start,
 * and caches it (I1: nothing native while FG is off).
 */
interface FgCapabilities {
  /** The process runs a 64-bit ABI that ships `libtatakai_fg.so` (arm64-v8a or x86_64). */
  val abiOk: Boolean

  /**
   * Amendment 1: highest Vulkan level the device declares through `PackageManager.FEATURE_VULKAN_HARDWARE_VERSION`, encoded as
   * [FgPolicy.VULKAN_1_1] / [FgPolicy.VULKAN_1_2] / [FgPolicy.VULKAN_1_3]; 0 = no Vulkan or below 1.1.
   */
  val vulkanLevel: Int

  /**
   * Amendment 1: the level to judge eligibility with. Once [FgNative.deviceInfo] has been probed ([probed] non-null), its
   * `apiVersion` has priority over [vulkanLevel] ([FgPolicy.effectiveVulkanLevel]); before the probe, [vulkanLevel].
   */
  fun vulkanLevel(probed: FgDeviceInfo?): Int = FgPolicy.effectiveVulkanLevel(vulkanLevel, probed?.apiVersion)

  /** Refresh rate of the display's current physical mode in Hz (0 when unknown); the mode, not the app's overridden rate. */
  fun currentHz(): Float

  /** Refresh rates of the display modes with the current resolution (fact 4, Auto). */
  fun supportedHz(): List<Float>

  /** Tatakai: the system battery saver is on (FG suspends with [FgUnavailableReason.LOW_POWER]). Default false. */
  fun powerSaveMode(): Boolean = false
}

/** R13: [FgCapabilities] backed by [Build], [PackageManager] features, [PowerManager] and the default [Display]. */
class AndroidFgCapabilities(
  context: Context,
) : FgCapabilities {
  private val appContext = context.applicationContext

  override val abiOk: Boolean by lazy {
    android.os.Process.is64Bit() && Build.SUPPORTED_64_BIT_ABIS.firstOrNull()?.let { it in FG_ABIS } == true
  }

  override val vulkanLevel: Int by lazy {
    runCatching {
      val pm = appContext.packageManager
      VULKAN_LEVELS.firstOrNull { pm.hasSystemFeature(PackageManager.FEATURE_VULKAN_HARDWARE_VERSION, it) } ?: 0
    }.getOrDefault(0)
  }

  private fun display(): Display? =
    runCatching {
      (appContext.getSystemService(Context.DISPLAY_SERVICE) as? DisplayManager)?.getDisplay(Display.DEFAULT_DISPLAY)
    }.getOrNull()

  // Amendment 3 (E20): physical mode, as supportedHz (Display.refreshRate is the app's overridden rate).
  override fun currentHz(): Float = runCatching { display()?.mode?.refreshRate }.getOrNull()?.takeIf { it > 0f } ?: 0f

  override fun supportedHz(): List<Float> =
    runCatching {
      val display = display() ?: return@runCatching emptyList()
      val current = display.mode
      display.supportedModes
        .filter { it.physicalWidth == current.physicalWidth && it.physicalHeight == current.physicalHeight }
        .map { it.refreshRate }
        .filter { it > 0f }
        .distinct()
    }.getOrDefault(emptyList())

  override fun powerSaveMode(): Boolean =
    runCatching { (appContext.getSystemService(Context.POWER_SERVICE) as? PowerManager)?.isPowerSaveMode == true }
      .getOrDefault(false)

  private companion object {
    val FG_ABIS = setOf("arm64-v8a", "x86_64")

    /** Probed from highest to lowest. */
    val VULKAN_LEVELS = listOf(FgPolicy.VULKAN_1_3, FgPolicy.VULKAN_1_2, FgPolicy.VULKAN_1_1)
  }
}
