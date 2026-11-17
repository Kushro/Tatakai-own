/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

/**
 * R13: GPU capabilities reported by [FgNative.deviceInfo] (`efg::probeDeviceInfo`).
 *
 * The native side writes
 * `apiVersion=..;gpuName=..;vendor=..;device=..;ahb=0|1;swapchain=0|1;displayTiming=0|1;fp16=0|1`.
 * [apiVersion], [vendor] and [device] keep the raw text (their exact format is fixed by the native
 * probe); the capability flags are true only for `1`.
 *
 * @property ahb `VK_ANDROID_external_memory_android_hardware_buffer` usable.
 * @property swapchain `VK_KHR_swapchain` usable.
 * @property displayTiming `VK_GOOGLE_display_timing` available (optional).
 * @property fp16 FP16 shader path supported (optional).
 */
data class FgDeviceInfo(
  val apiVersion: String,
  val gpuName: String,
  val vendor: String,
  val device: String,
  val ahb: Boolean,
  val swapchain: Boolean,
  val displayTiming: Boolean,
  val fp16: Boolean,
) {
  companion object {
    private const val KEY_API_VERSION = "apiVersion"
    private const val KEY_GPU_NAME = "gpuName"
    private const val KEY_VENDOR = "vendor"
    private const val KEY_DEVICE = "device"
    private const val KEY_AHB = "ahb"
    private const val KEY_SWAPCHAIN = "swapchain"
    private const val KEY_DISPLAY_TIMING = "displayTiming"
    private const val KEY_FP16 = "fp16"

    /**
     * Parses the `k=v;...` text. Each entry splits at its first `=`; unknown keys are ignored.
     * Returns null for null/blank input or when `apiVersion` or `gpuName` is missing.
     */
    fun parse(text: String?): FgDeviceInfo? {
      if (text.isNullOrBlank()) return null
      val map = HashMap<String, String>()
      for (entry in text.split(';')) {
        val eq = entry.indexOf('=')
        if (eq <= 0) continue
        map[entry.substring(0, eq).trim()] = entry.substring(eq + 1).trim()
      }
      val api = map[KEY_API_VERSION] ?: return null
      val gpu = map[KEY_GPU_NAME] ?: return null
      fun flag(key: String): Boolean = map[key] == "1"
      return FgDeviceInfo(
        apiVersion = api,
        gpuName = gpu,
        vendor = map[KEY_VENDOR].orEmpty(),
        device = map[KEY_DEVICE].orEmpty(),
        ahb = flag(KEY_AHB),
        swapchain = flag(KEY_SWAPCHAIN),
        displayTiming = flag(KEY_DISPLAY_TIMING),
        fp16 = flag(KEY_FP16),
      )
    }
  }
}
