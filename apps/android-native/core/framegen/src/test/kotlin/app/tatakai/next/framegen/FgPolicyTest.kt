/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 */

package app.tatakai.next.framegen

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.min

class FgPolicyTest {
  // ---------------------------------------------------------------- eligibility (facts 13, 16, 22)

  private fun elig(
    abiOk: Boolean = true,
    libLoaded: Boolean = true,
    vulkanLevel: Int = FgPolicy.VULKAN_1_3,
    allowUntested: Boolean = false,
    gpuName: String? = "Adreno (TM) 740",
    shadersReady: Boolean = true,
    inPip: Boolean = false,
    background: Boolean = false,
    lowPower: Boolean = false,
    eightK: Boolean = false,
    hdrPassthrough: Boolean = false,
    remote: Boolean = false,
  ) = FgPolicy.eligibility(
    abiOk, libLoaded, vulkanLevel, allowUntested, gpuName, shadersReady, inPip, background, lowPower, eightK,
    hdrPassthrough, remote,
  )

  @Test
  fun `Temple - remote is the first suspension, after every ineligible reason`() {
    assertEquals(FgUnavailableReason.Kind.SUSPENDED, FgUnavailableReason.REMOTE.kind)
    assertEquals(FgEligibility.Suspended(FgUnavailableReason.REMOTE), elig(remote = true))
    assertEquals(
      FgEligibility.Suspended(FgUnavailableReason.REMOTE),
      elig(remote = true, inPip = true, background = true, lowPower = true, eightK = true, hdrPassthrough = true),
    )
    assertEquals(FgEligibility.Ineligible(FgUnavailableReason.ABI), elig(remote = true, abiOk = false))
    assertEquals(FgEligibility.Ineligible(FgUnavailableReason.SHADERS), elig(remote = true, shadersReady = false))
    // The default keeps the previous rule for every caller that does not pass it.
    assertEquals(FgEligibility.Suspended(FgUnavailableReason.PIP), elig(inPip = true))
    assertEquals(FgEligibility.Eligible(experimental = false), elig(remote = false))
  }

  @Test
  fun `eligible when everything is ok`() {
    assertEquals(FgEligibility.Eligible(experimental = false), elig())
    assertEquals(FgEligibility.Eligible(experimental = true), elig(gpuName = "Mali-G715"))
    // REV 8: an unknown (not yet probed) GPU is not experimental by itself.
    assertEquals(FgEligibility.Eligible(experimental = false), elig(gpuName = null))
  }

  // ---------------------------------------------------------------- untested Vulkan switch (Amendment 1)

  private val vulkan10 = 0x00400000

  @Test
  fun `switch off keeps the Vulkan 1_3 rule`() {
    for (level in listOf(0, vulkan10, FgPolicy.VULKAN_1_1, 0x00401080, FgPolicy.VULKAN_1_2)) {
      assertEquals(
        "level 0x${level.toString(16)}",
        FgEligibility.Ineligible(FgUnavailableReason.VULKAN),
        elig(vulkanLevel = level, allowUntested = false),
      )
    }
    assertEquals(FgEligibility.Eligible(experimental = false), elig(vulkanLevel = FgPolicy.VULKAN_1_3))
    assertEquals(FgEligibility.Eligible(experimental = false), elig(vulkanLevel = 0x00403104))
  }

  @Test
  fun `switch on makes Vulkan 1_1 and 1_2 eligible and always experimental`() {
    for (level in listOf(FgPolicy.VULKAN_1_1, 0x00401080, FgPolicy.VULKAN_1_2)) {
      for (gpu in listOf("Adreno (TM) 740", "Adreno (TM) 619", null)) {
        assertEquals(
          "level 0x${level.toString(16)} gpu $gpu",
          FgEligibility.Eligible(experimental = true),
          elig(vulkanLevel = level, allowUntested = true, gpuName = gpu),
        )
      }
    }
    assertEquals(
      FgEligibility.Eligible(experimental = false),
      elig(vulkanLevel = FgPolicy.VULKAN_1_3, allowUntested = true),
    )
  }

  @Test
  fun `switch on still rejects Vulkan 1_0 and no Vulkan`() {
    for (level in listOf(0, vulkan10, FgPolicy.VULKAN_1_1 - 1)) {
      assertEquals(
        FgEligibility.Ineligible(FgUnavailableReason.VULKAN),
        elig(vulkanLevel = level, allowUntested = true),
      )
    }
  }

  @Test
  fun `experimental flag with Vulkan 1_3 depends only on a known GPU`() {
    assertEquals(FgEligibility.Eligible(experimental = false), elig(vulkanLevel = FgPolicy.VULKAN_1_3, gpuName = null))
    assertEquals(
      FgEligibility.Eligible(experimental = true),
      elig(vulkanLevel = FgPolicy.VULKAN_1_3, gpuName = "Adreno (TM) 619"),
    )
    assertEquals(
      FgEligibility.Eligible(experimental = true),
      elig(vulkanLevel = FgPolicy.VULKAN_1_3, allowUntested = true, gpuName = "Adreno (TM) 619"),
    )
  }

  @Test
  fun `switch on keeps the priority order of reasons`() {
    val e = elig(vulkanLevel = FgPolicy.VULKAN_1_1, allowUntested = true, shadersReady = false, inPip = true)
    assertEquals(FgEligibility.Ineligible(FgUnavailableReason.SHADERS), e)
    assertEquals(
      FgEligibility.Suspended(FgUnavailableReason.PIP),
      elig(vulkanLevel = FgPolicy.VULKAN_1_2, allowUntested = true, inPip = true),
    )
    assertEquals(
      FgEligibility.Ineligible(FgUnavailableReason.LIBRARY),
      elig(vulkanLevel = 0, allowUntested = true, libLoaded = false),
    )
  }

  @Test
  fun `I10 switch off matches the pre-amendment rule except REV 8`() {
    val levels = listOf(0, vulkan10, FgPolicy.VULKAN_1_1, FgPolicy.VULKAN_1_2, FgPolicy.VULKAN_1_3, 0x00403104)
    val gpus = listOf("Adreno (TM) 740", "Adreno (TM) 619", "Mali-G715", null)
    val bools = listOf(false, true)
    for (level in levels) for (gpu in gpus) for (abi in bools) for (lib in bools) for (sh in bools) for (pip in bools) {
      val legacyVulkanOk = level >= FgPolicy.VULKAN_1_3
      // The rule before Amendment 1, written out.
      val legacy = when {
        !abi -> FgEligibility.Ineligible(FgUnavailableReason.ABI)
        !lib -> FgEligibility.Ineligible(FgUnavailableReason.LIBRARY)
        !legacyVulkanOk -> FgEligibility.Ineligible(FgUnavailableReason.VULKAN)
        !sh -> FgEligibility.Ineligible(FgUnavailableReason.SHADERS)
        pip -> FgEligibility.Suspended(FgUnavailableReason.PIP)
        // REV 8 is the only difference: null GPU → not experimental.
        else -> FgEligibility.Eligible(experimental = gpu != null && FgPolicy.isExperimentalGpu(gpu))
      }
      assertEquals(
        "level=0x${level.toString(16)} gpu=$gpu abi=$abi lib=$lib sh=$sh pip=$pip",
        legacy,
        elig(
          abiOk = abi, libLoaded = lib, vulkanLevel = level, allowUntested = false,
          gpuName = gpu, shadersReady = sh, inPip = pip,
        ),
      )
    }
  }

  @Test
  fun `vulkan level constants match the Vulkan encoding`() {
    assertEquals(0x00401000, FgPolicy.VULKAN_1_1)
    assertEquals(0x00402000, FgPolicy.VULKAN_1_2)
    assertEquals(0x00403000, FgPolicy.VULKAN_1_3)
    assertTrue(FgPolicy.VULKAN_1_1 < FgPolicy.VULKAN_1_2 && FgPolicy.VULKAN_1_2 < FgPolicy.VULKAN_1_3)
  }

  @Test
  fun `probed apiVersion has priority over the declared level`() {
    assertEquals(0x00401080, FgPolicy.parseApiVersion("1.1.128"))
    assertEquals(FgPolicy.VULKAN_1_3, FgPolicy.parseApiVersion("1.3.0"))
    assertEquals(FgPolicy.VULKAN_1_2, FgPolicy.parseApiVersion("1.2"))
    assertNull(FgPolicy.parseApiVersion(null))
    assertNull(FgPolicy.parseApiVersion(""))
    assertNull(FgPolicy.parseApiVersion("vulkan"))
    assertNull(FgPolicy.parseApiVersion("1.1.99999"))
    assertEquals(0x00401080, FgPolicy.effectiveVulkanLevel(FgPolicy.VULKAN_1_3, "1.1.128"))
    assertEquals(FgPolicy.VULKAN_1_3 + 250, FgPolicy.effectiveVulkanLevel(FgPolicy.VULKAN_1_1, "1.3.250"))
    assertEquals(FgPolicy.VULKAN_1_2, FgPolicy.effectiveVulkanLevel(FgPolicy.VULKAN_1_2, null))
    assertEquals(FgPolicy.VULKAN_1_2, FgPolicy.effectiveVulkanLevel(FgPolicy.VULKAN_1_2, "garbage"))
  }

  @Test
  fun `each single failing condition maps to its reason`() {
    val table = listOf(
      elig(abiOk = false) to FgEligibility.Ineligible(FgUnavailableReason.ABI),
      elig(libLoaded = false) to FgEligibility.Ineligible(FgUnavailableReason.LIBRARY),
      elig(vulkanLevel = 0) to FgEligibility.Ineligible(FgUnavailableReason.VULKAN),
      elig(shadersReady = false) to FgEligibility.Ineligible(FgUnavailableReason.SHADERS),
      elig(inPip = true) to FgEligibility.Suspended(FgUnavailableReason.PIP),
      elig(background = true) to FgEligibility.Suspended(FgUnavailableReason.BACKGROUND),
      elig(lowPower = true) to FgEligibility.Suspended(FgUnavailableReason.LOW_POWER),
      elig(eightK = true) to FgEligibility.Suspended(FgUnavailableReason.EIGHT_K),
      elig(hdrPassthrough = true) to FgEligibility.Suspended(FgUnavailableReason.HDR_PASSTHROUGH),
    )
    for ((actual, expected) in table) assertEquals(expected, actual)
  }

  @Test
  fun `reason kinds match the eligibility class`() {
    for (r in listOf(FgUnavailableReason.ABI, FgUnavailableReason.LIBRARY, FgUnavailableReason.VULKAN, FgUnavailableReason.SHADERS)) {
      assertEquals(FgUnavailableReason.Kind.INELIGIBLE, r.kind)
    }
    for (r in listOf(
      FgUnavailableReason.PIP, FgUnavailableReason.BACKGROUND, FgUnavailableReason.LOW_POWER,
      FgUnavailableReason.EIGHT_K, FgUnavailableReason.HDR_PASSTHROUGH,
    )) {
      assertEquals(FgUnavailableReason.Kind.SUSPENDED, r.kind)
    }
    for (r in listOf(FgUnavailableReason.DEVICE_LOST, FgUnavailableReason.START_FAILED)) {
      assertEquals(FgUnavailableReason.Kind.FAILED, r.kind)
    }
  }

  @Test
  fun `ineligible reasons win in order ABI LIBRARY VULKAN SHADERS`() {
    val allSuspended = { abi: Boolean, lib: Boolean, vk: Boolean, sh: Boolean ->
      elig(
        abiOk = abi, libLoaded = lib, vulkanLevel = if (vk) FgPolicy.VULKAN_1_3 else 0, shadersReady = sh,
        inPip = true, background = true, lowPower = true, eightK = true, hdrPassthrough = true,
      )
    }
    assertEquals(FgEligibility.Ineligible(FgUnavailableReason.ABI), allSuspended(false, false, false, false))
    assertEquals(FgEligibility.Ineligible(FgUnavailableReason.LIBRARY), allSuspended(true, false, false, false))
    assertEquals(FgEligibility.Ineligible(FgUnavailableReason.VULKAN), allSuspended(true, true, false, false))
    assertEquals(FgEligibility.Ineligible(FgUnavailableReason.SHADERS), allSuspended(true, true, true, false))
    assertEquals(FgEligibility.Suspended(FgUnavailableReason.PIP), allSuspended(true, true, true, true))
  }

  @Test
  fun `suspensions follow order PIP BACKGROUND LOW_POWER EIGHT_K HDR_PASSTHROUGH`() {
    assertEquals(
      FgEligibility.Suspended(FgUnavailableReason.BACKGROUND),
      elig(background = true, lowPower = true, eightK = true, hdrPassthrough = true),
    )
    assertEquals(
      FgEligibility.Suspended(FgUnavailableReason.LOW_POWER),
      elig(lowPower = true, eightK = true, hdrPassthrough = true),
    )
    assertEquals(FgEligibility.Suspended(FgUnavailableReason.EIGHT_K), elig(eightK = true, hdrPassthrough = true))
  }

  @Test
  fun `experimental gpu only outside Adreno 7xx and above`() {
    val table = mapOf(
      "Adreno (TM) 740" to false,
      "Adreno (TM) 750" to false,
      "adreno 830" to false,
      "ADRENO (TM) 700" to false,
      "Adreno (TM) 699" to true,
      "Adreno 650" to true,
      "Adreno (TM) 619" to true,
      "Mali-G715" to true,
      "Mali-G710 MC10" to true,
      "PowerVR B-Series BXM-8-256" to true,
      "Adreno" to true,
      "" to true,
    )
    for ((name, expected) in table) assertEquals(name, expected, FgPolicy.isExperimentalGpu(name))
    assertTrue(FgPolicy.isExperimentalGpu(null))
  }

  // ---------------------------------------------------------------- Auto multiplier (fact 4)

  private data class AutoRow(val fps: Double, val currentHz: Float, val n: Int, val targetHz: Float, val exact: Boolean)

  /** Display modes used for every «mode switch on» row. */
  private val supported = listOf(60f, 72f, 90f, 120f, 144f)

  /**
   * Mode switch ON, supported = [60, 72, 90, 120, 144]: smallest N whose fps×N matches any mode;
   * no match → largest N with fps×N ≤ current (min 2), target = current, exact = false.
   */
  private val autoSwitchOn = listOf(
    // fps      cur    N  target exact
    AutoRow(23.976, 60f, 3, 72f, true), // 71.93 ≈ 72
    AutoRow(23.976, 90f, 3, 72f, true),
    AutoRow(23.976, 120f, 3, 72f, true),
    AutoRow(23.976, 144f, 3, 72f, true),
    AutoRow(24.0, 60f, 3, 72f, true), // 72
    AutoRow(24.0, 90f, 3, 72f, true),
    AutoRow(24.0, 120f, 3, 72f, true),
    AutoRow(24.0, 144f, 3, 72f, true),
    AutoRow(25.0, 60f, 2, 60f, false), // 50/75/100/125 match no mode; 50 ≤ 60 < 75
    AutoRow(25.0, 90f, 3, 90f, false), // 75 ≤ 90 < 100
    AutoRow(25.0, 120f, 4, 120f, false), // 100 ≤ 120 < 125
    AutoRow(25.0, 144f, 5, 144f, false), // 125 ≤ 144
    AutoRow(29.97, 60f, 2, 60f, true), // 59.94 ≈ 60
    AutoRow(29.97, 90f, 2, 60f, true),
    AutoRow(29.97, 120f, 2, 60f, true),
    AutoRow(29.97, 144f, 2, 60f, true),
    AutoRow(30.0, 60f, 2, 60f, true),
    AutoRow(30.0, 90f, 2, 60f, true),
    AutoRow(30.0, 120f, 2, 60f, true),
    AutoRow(30.0, 144f, 2, 60f, true),
    AutoRow(50.0, 60f, 2, 60f, false), // 100/150/200/250 match no mode; 100 > 60 → min 2
    AutoRow(50.0, 90f, 2, 90f, false),
    AutoRow(50.0, 120f, 2, 120f, false), // 100 ≤ 120 < 150
    AutoRow(50.0, 144f, 2, 144f, false), // 100 ≤ 144 < 150
    AutoRow(60.0, 60f, 2, 120f, true), // 120
    AutoRow(60.0, 90f, 2, 120f, true),
    AutoRow(60.0, 120f, 2, 120f, true),
    AutoRow(60.0, 144f, 2, 120f, true),
  )

  /**
   * Mode switch OFF: smallest N whose fps×N matches the current refresh («divisor exacto»);
   * no match → largest N with fps×N ≤ current (min 2). Target is always the current refresh.
   */
  private val autoSwitchOff = listOf(
    // 60 Hz
    AutoRow(23.976, 60f, 2, 60f, false), // 47.95 ≤ 60 < 71.93
    AutoRow(24.0, 60f, 2, 60f, false), // 48 ≤ 60 < 72
    AutoRow(25.0, 60f, 2, 60f, false), // 50 ≤ 60 < 75
    AutoRow(29.97, 60f, 2, 60f, true), // 59.94 ≈ 60
    AutoRow(30.0, 60f, 2, 60f, true), // 60
    AutoRow(50.0, 60f, 2, 60f, false), // 100 > 60 → min 2
    AutoRow(60.0, 60f, 2, 60f, false), // 120 > 60 → min 2
    // 90 Hz
    AutoRow(23.976, 90f, 3, 90f, false), // 71.93 ≤ 90 < 95.90
    AutoRow(24.0, 90f, 3, 90f, false), // 72 ≤ 90 < 96
    AutoRow(25.0, 90f, 3, 90f, false), // 75 ≤ 90 < 100
    AutoRow(29.97, 90f, 3, 90f, true), // 89.91 ≈ 90
    AutoRow(30.0, 90f, 3, 90f, true), // 90
    AutoRow(50.0, 90f, 2, 90f, false), // 100 > 90 → min 2
    AutoRow(60.0, 90f, 2, 90f, false), // 120 > 90 → min 2
    // 120 Hz
    AutoRow(23.976, 120f, 5, 120f, true), // 119.88 ≈ 120
    AutoRow(24.0, 120f, 5, 120f, true), // 120
    AutoRow(25.0, 120f, 4, 120f, false), // 100 ≤ 120 < 125
    AutoRow(29.97, 120f, 4, 120f, true), // 119.88 ≈ 120
    AutoRow(30.0, 120f, 4, 120f, true), // 120
    AutoRow(50.0, 120f, 2, 120f, false), // 100 ≤ 120 < 150
    AutoRow(60.0, 120f, 2, 120f, true), // 120
    // 144 Hz
    AutoRow(23.976, 144f, 5, 144f, false), // 119.88 ≤ 144 (N max 5)
    AutoRow(24.0, 144f, 5, 144f, false), // 120 ≤ 144
    AutoRow(25.0, 144f, 5, 144f, false), // 125 ≤ 144
    AutoRow(29.97, 144f, 4, 144f, false), // 119.88 ≤ 144 < 149.85
    AutoRow(30.0, 144f, 4, 144f, false), // 120 ≤ 144 < 150
    AutoRow(50.0, 144f, 2, 144f, false), // 100 ≤ 144 < 150
    AutoRow(60.0, 144f, 2, 144f, false), // 120 ≤ 144 < 180
  )

  private fun checkAuto(rows: List<AutoRow>, allowModeSwitch: Boolean) {
    assertEquals(28, rows.size) // 7 fps × 4 refresh rates
    for (r in rows) {
      assertEquals(
        "fps=${r.fps} current=${r.currentHz} switch=$allowModeSwitch",
        AutoChoice(r.n, r.targetHz, r.exact),
        FgPolicy.autoMultiplier(r.fps, r.currentHz, supported, allowModeSwitch),
      )
    }
  }

  @Test
  fun `auto table with mode switch on`() = checkAuto(autoSwitchOn, allowModeSwitch = true)

  @Test
  fun `auto table with mode switch off`() = checkAuto(autoSwitchOff, allowModeSwitch = false)

  @Test
  fun `auto literal examples of fact 4`() {
    // Switch on: 24 → 72 or 120; 30 → 60 or 120.
    assertEquals(AutoChoice(3, 72f, true), FgPolicy.autoMultiplier(24.0, 60f, listOf(60f, 72f, 120f), true))
    assertEquals(AutoChoice(5, 120f, true), FgPolicy.autoMultiplier(24.0, 60f, listOf(60f, 120f), true))
    assertEquals(AutoChoice(2, 60f, true), FgPolicy.autoMultiplier(30.0, 120f, listOf(60f, 120f), true))
    assertEquals(AutoChoice(4, 120f, true), FgPolicy.autoMultiplier(30.0, 120f, listOf(120f), true))
    // Switch off: 24 @ 120 Hz → 5×; 30 @ 60 Hz → 2×.
    assertEquals(AutoChoice(5, 120f, true), FgPolicy.autoMultiplier(24.0, 120f, supported, false))
    assertEquals(AutoChoice(2, 60f, true), FgPolicy.autoMultiplier(30.0, 60f, supported, false))
    // Fallback minimum 2×: 50 fps @ 60 Hz, no mode matches.
    assertEquals(AutoChoice(2, 60f, false), FgPolicy.autoMultiplier(50.0, 60f, listOf(60f), true))
    assertEquals(AutoChoice(2, 60f, false), FgPolicy.autoMultiplier(50.0, 60f, emptyList(), false))
  }

  @Test
  fun `auto with switch on falls back to the current refresh rule when no mode matches`() {
    // Only 60 and 90 Hz: 24 fps matches neither (48/72/96/120) → largest N with 24N ≤ 90.
    assertEquals(AutoChoice(3, 90f, false), FgPolicy.autoMultiplier(24.0, 90f, listOf(60f, 90f), true))
    // The current refresh counts as a supported mode even if missing from the list.
    assertEquals(AutoChoice(5, 120f, true), FgPolicy.autoMultiplier(24.0, 120f, emptyList(), true))
  }

  @Test
  fun `auto tolerance is half a percent`() {
    // 24 × 3 = 72 vs 72.3 Hz: 0.3 ≤ 0.3615 → match; vs 72.5 Hz: 0.5 > 0.3625 → no match,
    // fallback keeps 3× because 72 ≤ 72.5 but exact = false.
    assertEquals(AutoChoice(3, 72.3f, true), FgPolicy.autoMultiplier(24.0, 72.3f, emptyList(), false))
    assertEquals(AutoChoice(3, 72.5f, false), FgPolicy.autoMultiplier(24.0, 72.5f, emptyList(), false))
  }

  @Test
  fun `auto with invalid fps returns 2x on the current refresh`() {
    for (fps in listOf(0.0, -24.0, Double.NaN, Double.POSITIVE_INFINITY)) {
      assertEquals("fps=$fps", AutoChoice(2, 120f, false), FgPolicy.autoMultiplier(fps, 120f, supported, true))
    }
  }

  @Test
  fun `auto ignores invalid supported modes`() {
    assertEquals(
      AutoChoice(2, 60f, false),
      FgPolicy.autoMultiplier(24.0, 60f, listOf(0f, -72f, Float.NaN, Float.POSITIVE_INFINITY), true),
    )
  }

  @Test
  fun `auto maxN limits the multiplier`() {
    val modes = listOf(60f, 120f)
    // 24 fps: 48/72 match no mode and not 120 Hz → largest N ≤ 3 with 24N ≤ 120.
    assertEquals(AutoChoice(3, 120f, false), FgPolicy.autoMultiplier(24.0, 120f, modes, true, maxN = 3))
    assertEquals(AutoChoice(2, 120f, false), FgPolicy.autoMultiplier(24.0, 120f, modes, true, maxN = 2))
    // Without the cap it is 5× on 120 Hz.
    assertEquals(AutoChoice(5, 120f, true), FgPolicy.autoMultiplier(24.0, 120f, modes, true))
    // 30 fps: 2× matches 60 Hz, within the cap.
    assertEquals(AutoChoice(2, 60f, true), FgPolicy.autoMultiplier(30.0, 60f, modes, true, maxN = 3))
    assertEquals(
      AutoChoice(2, 60f, true),
      FgPolicy.autoMultiplier(30.0, 60f, modes, true, maxN = FgPolicy.AUTO_EXPERIMENTAL_MAX_MULTIPLIER),
    )
    // Switch off with the cap: 24 @ 120 Hz would be 5× exact; capped to 3 → 3×, not exact.
    assertEquals(AutoChoice(3, 120f, false), FgPolicy.autoMultiplier(24.0, 120f, supported, false, maxN = 3))
  }

  @Test
  fun `auto maxN out of range is clamped`() {
    val modes = listOf(60f, 120f)
    for (maxN in listOf(1, 0, -3, Int.MIN_VALUE)) {
      assertEquals("maxN=$maxN", AutoChoice(2, 120f, false), FgPolicy.autoMultiplier(24.0, 120f, modes, true, maxN))
    }
    for (maxN in listOf(6, 9, Int.MAX_VALUE)) {
      assertEquals("maxN=$maxN", AutoChoice(5, 120f, true), FgPolicy.autoMultiplier(24.0, 120f, modes, true, maxN))
    }
  }

  @Test
  fun `auto default maxN keeps both tables`() {
    for (r in autoSwitchOn) {
      assertEquals(
        AutoChoice(r.n, r.targetHz, r.exact),
        FgPolicy.autoMultiplier(r.fps, r.currentHz, supported, true, FgPolicy.MAX_MULTIPLIER),
      )
    }
    for (r in autoSwitchOff) {
      assertEquals(
        AutoChoice(r.n, r.targetHz, r.exact),
        FgPolicy.autoMultiplier(r.fps, r.currentHz, supported, false, FgPolicy.MAX_MULTIPLIER),
      )
    }
  }

  // ---------------------------------------------------------------- resolveProfile (fact 25)

  @Test
  fun `presets ignore the custom values`() {
    val custom = FgTuning(performanceMode = false, flowScale = 0.3f, inputScale = 0.75f)
    assertEquals(FgTuning.SAVER, FgPolicy.resolveProfile(FgProfile.SAVER, custom))
    assertEquals(FgTuning.BALANCED, FgPolicy.resolveProfile(FgProfile.BALANCED, custom))
    assertEquals(FgTuning.QUALITY, FgPolicy.resolveProfile(FgProfile.QUALITY, custom))
  }

  @Test
  fun `custom keeps valid values`() {
    val custom = FgTuning(performanceMode = false, flowScale = 0.3f, inputScale = 0.75f)
    assertEquals(custom, FgPolicy.resolveProfile(FgProfile.CUSTOM, custom))
  }

  @Test
  fun `custom out of range is bounded`() {
    assertEquals(
      FgTuning(performanceMode = true, flowScale = 0.25f, inputScale = 0.75f),
      FgPolicy.resolveProfile(FgProfile.CUSTOM, FgTuning(true, flowScale = 0.1f, inputScale = 0.5f)),
    )
    assertEquals(
      FgTuning(performanceMode = true, flowScale = 1.0f, inputScale = 1.0f),
      FgPolicy.resolveProfile(FgProfile.CUSTOM, FgTuning(true, flowScale = 3f, inputScale = 2f)),
    )
    assertEquals(
      FgTuning(performanceMode = false, flowScale = 0.5f, inputScale = 1.0f),
      FgPolicy.resolveProfile(FgProfile.CUSTOM, FgTuning(false, flowScale = Float.NaN, inputScale = Float.NaN)),
    )
    for (p in FgProfile.entries) {
      assertTrue(p.name, FgPolicy.resolveProfile(p, FgTuning(true, 9f, 9f)).isValidCustom())
    }
  }

  // ---------------------------------------------------------------- targetFrameRate

  @Test
  fun `target frame rate is fps times n only with match refresh and valid fps`() {
    assertEquals(72.0, FgPolicy.targetFrameRate(24.0, 3, true)!!, 1e-9)
    assertEquals(119.88, FgPolicy.targetFrameRate(23.976, 5, true)!!, 1e-9)
    assertNull(FgPolicy.targetFrameRate(24.0, 3, false))
    assertNull(FgPolicy.targetFrameRate(null, 3, true))
    assertNull(FgPolicy.targetFrameRate(0.0, 3, true))
    assertNull(FgPolicy.targetFrameRate(-1.0, 3, true))
    assertNull(FgPolicy.targetFrameRate(Double.NaN, 3, true))
    assertNull(FgPolicy.targetFrameRate(24.0, 1, true))
    assertNull(FgPolicy.targetFrameRate(24.0, 6, true))
  }

  // ---------------------------------------------------------------- inputSize

  @Test
  fun `input size is even and at least 2`() {
    assertEquals(1920 to 1080, FgPolicy.inputSize(1920, 1080, 1.0f))
    assertEquals(1440 to 810, FgPolicy.inputSize(1920, 1080, 0.75f))
    assertEquals(1024 to 576, FgPolicy.inputSize(1366, 768, 0.75f)) // 1024.5 → 1024 (round half up 1025 → even 1024)
    assertEquals(2400 to 1080, FgPolicy.inputSize(2400, 1080, 1.0f))
    assertEquals(1000 to 600, FgPolicy.inputSize(1001, 601, 1.0f)) // odd → lower even
    assertEquals(2 to 2, FgPolicy.inputSize(1, 1, 1.0f))
    assertEquals(2 to 2, FgPolicy.inputSize(0, -5, 0.75f))
    assertEquals(1920 to 1080, FgPolicy.inputSize(1920, 1080, Float.NaN))
    assertEquals(1920 to 1080, FgPolicy.inputSize(1920, 1080, 0f))
  }

  // ---------------------------------------------------------------- inputSize limited to the video (E16, E16b)

  // FIX-5 changed the default floor from 0 to 1.0: the rows written for the strict rule now pass
  // floor = 0.0 explicitly; the default is checked in `default floor 1_0 never goes below the video`.

  @Test
  fun `input size follows the video on a 2460x1080 output`() {
    // kIn = min(1920/2460, 1080/1080) = 0.7805 → 1920 × 842.9 → 1920×842.
    assertEquals(1920 to 842, FgPolicy.inputSize(2460, 1080, 1f, 1920 to 1080, floor = 0.0))
    // kIn = min(0.5203, 0.6667) = 0.5203 → 1280 × 561.95 → 1280×562.
    assertEquals(1280 to 562, FgPolicy.inputSize(2460, 1080, 1f, 1280 to 720, floor = 0.0))
    // k = min(0.75, 0.7805) = 0.75 → 1845 → 1844, 810.
    assertEquals(1844 to 810, FgPolicy.inputSize(2460, 1080, 0.75f, 1920 to 1080, floor = 0.0))
    // A video larger than the output never goes above it: k = min(1, 1.56) = 1.
    assertEquals(2460 to 1080, FgPolicy.inputSize(2460, 1080, 1f, 3840 to 2160, floor = 0.0))
    // Small video: kIn = 0.13, kMin = 360/1080 → 820×360.
    assertEquals(820 to 360, FgPolicy.inputSize(2460, 1080, 1f, 320 to 240, floor = 0.0))
  }

  @Test
  fun `default floor 1_0 never goes below the video`() {
    assertEquals(1.0, FgPolicy.VIDEO_FIT_FLOOR_DEFAULT, 0.0)
    // kOut = min(1, max(0.5203, 0.6667)) = 0.6667 → 1640 × 720: covers the 1280×720 video.
    assertEquals(1640 to 720, FgPolicy.inputSize(2460, 1080, 1f, 1280 to 720))
    // kOut = min(1, max(0.7805, 1)) = 1 → the whole output.
    assertEquals(2460 to 1080, FgPolicy.inputSize(2460, 1080, 1f, 1920 to 1080))
    // Portrait video: kOut = 1 → the whole output (it was 1080×474 with the strict floor).
    assertEquals(2460 to 1080, FgPolicy.inputSize(2460, 1080, 1f, 1080 to 1920))
    // The scale still caps it: min(0.75, 1) → 1844×810.
    assertEquals(1844 to 810, FgPolicy.inputSize(2460, 1080, 0.75f, 1920 to 1080))
    // Small video: kOut = 0.2222 < kMin = 0.3333 → 820×360, as before.
    assertEquals(820 to 360, FgPolicy.inputSize(2460, 1080, 1f, 320 to 240))
  }

  @Test
  fun `input size for a portrait video depends on the floor`() {
    // kIn = min(1080/2460, 1920/1080) = 0.4390, kOut = 1, kMin = 0.3333.
    // Floor 0: k = 0.4390 → 1080 × 474.1 → 1080×474.
    assertEquals(1080 to 474, FgPolicy.inputSize(2460, 1080, 1f, 1080 to 1920, floor = 0.0))
    // Floor 1.0 (the default): k = 1 → 2460×1080.
    assertEquals(2460 to 1080, FgPolicy.inputSize(2460, 1080, 1f, 1080 to 1920, floor = 1.0))
    // Floor 0.5: k = 0.5 → 1230 × 540.
    assertEquals(1230 to 540, FgPolicy.inputSize(2460, 1080, 1f, 1080 to 1920, floor = 0.5))
    // Floor 0.75: k = 0.75 → 1845 → 1844, 810.
    assertEquals(1844 to 810, FgPolicy.inputSize(2460, 1080, 1f, 1080 to 1920, floor = 0.75))
    // The floor never goes above the scale: min(0.75, …).
    assertEquals(1844 to 810, FgPolicy.inputSize(2460, 1080, 0.75f, 1080 to 1920, floor = 0.75))
  }

  @Test
  fun `input size on a portrait output`() {
    // Output 1080×2460, video 1080×1920: kIn = min(1, 0.7805) → 842.9 → 842, 1920.
    assertEquals(842 to 1920, FgPolicy.inputSize(1080, 2460, 1f, 1080 to 1920, floor = 0.0))
    // Default floor 1.0: kOut = min(1, max(1, 0.7805)) = 1 → the whole output.
    assertEquals(1080 to 2460, FgPolicy.inputSize(1080, 2460, 1f, 1080 to 1920))
  }

  @Test
  fun `input size without a video equals the three argument overload`() {
    for ((w, h) in listOf(2460 to 1080, 1920 to 1080, 1366 to 768, 1001 to 601, 1 to 1, 0 to -5)) {
      for (s in listOf(1f, 0.75f, Float.NaN, 0f)) {
        assertEquals("$w×$h s=$s", FgPolicy.inputSize(w, h, s), FgPolicy.inputSize(w, h, s, null))
        assertEquals("$w×$h s=$s", FgPolicy.inputSize(w, h, s), FgPolicy.inputSize(w, h, s, null, 0.75))
      }
    }
    // Invalid output or video sides also fall back to the three argument overload.
    assertEquals(FgPolicy.inputSize(0, 1080, 1f), FgPolicy.inputSize(0, 1080, 1f, 1920 to 1080))
    assertEquals(FgPolicy.inputSize(2460, 1080, 1f), FgPolicy.inputSize(2460, 1080, 1f, 0 to 1080))
    // An invalid scale counts as 1.
    assertEquals(1920 to 842, FgPolicy.inputSize(2460, 1080, Float.NaN, 1920 to 1080, floor = 0.0))
  }

  @Test
  fun `input size properties even sides within the output and same aspect`() {
    val outputs = listOf(2460 to 1080, 1080 to 2460, 1920 to 1080, 2400 to 1080, 1600 to 720, 1280 to 800)
    val videos = listOf(
      1920 to 1080, 1280 to 720, 3840 to 2160, 1080 to 1920, 720 to 1280, 640 to 480, 320 to 240,
      1440 to 1080, 4096 to 1716, 854 to 480, 2 to 2,
    )
    for ((ow, oh) in outputs) for ((vw, vh) in videos) for (s in listOf(1f, 0.75f)) {
      for (floor in listOf(0.0, 0.5, 0.75, 1.0)) {
        val (w, h) = FgPolicy.inputSize(ow, oh, s, vw to vh, floor)
        val msg = "out=$ow×$oh video=$vw×$vh s=$s floor=$floor → $w×$h"
        assertEquals(msg, 0, w % 2)
        assertEquals(msg, 0, h % 2)
        assertTrue(msg, w in 2..ow && h in 2..oh)
        // Aspect: the short side predicted from the long side, within 2 px.
        val (inLong, inShort, outLong, outShort) =
          if (ow >= oh) listOf(w, h, ow, oh) else listOf(h, w, oh, ow)
        val predicted = inLong.toDouble() * outShort / outLong
        assertTrue(msg, kotlin.math.abs(inShort - predicted) <= 2.0)
      }
    }
  }

  @Test
  fun `video display size swaps sides when rotated by 90`() {
    assertEquals(1080 to 1920, FgPolicy.videoDisplaySize(1920, 1080, 90))
    assertEquals(1080 to 1920, FgPolicy.videoDisplaySize(1920, 1080, 270))
    assertEquals(1920 to 1080, FgPolicy.videoDisplaySize(1920, 1080, 0))
    assertEquals(1920 to 1080, FgPolicy.videoDisplaySize(1920, 1080, 180))
    assertEquals(1920 to 1080, FgPolicy.videoDisplaySize(1920, 1080, null))
    assertNull(FgPolicy.videoDisplaySize(null, 1080, 0))
    assertNull(FgPolicy.videoDisplaySize(1920, null, 90))
    assertNull(FgPolicy.videoDisplaySize(0, 1080, 0))
    assertNull(FgPolicy.videoDisplaySize(1920, -1, 0))
  }

  @Test
  fun `video fit floor snaps to the allowed values`() {
    // FIX-5: 1.0 joins the list and is the default (it was 0).
    assertEquals(listOf(0f, 0.5f, 0.75f, 1f), FgPolicy.VIDEO_FIT_FLOORS)
    assertEquals(1.0, FgPolicy.VIDEO_FIT_FLOOR_DEFAULT, 0.0)
    assertEquals(0f, FgPolicy.snapVideoFitFloor(Float.NaN), 0f)
    assertEquals(0f, FgPolicy.snapVideoFitFloor(0f), 0f)
    assertEquals(0f, FgPolicy.snapVideoFitFloor(-1f), 0f)
    assertEquals(0.5f, FgPolicy.snapVideoFitFloor(0.6f), 0f)
    assertEquals(0.75f, FgPolicy.snapVideoFitFloor(0.7f), 0f)
    assertEquals(0.75f, FgPolicy.snapVideoFitFloor(0.75f), 0f)
    assertEquals(0.75f, FgPolicy.snapVideoFitFloor(0.85f), 0f)
    assertEquals(1f, FgPolicy.snapVideoFitFloor(0.9f), 0f)
    // Above the list it now snaps to 1.0 (it was 0.75 before FIX-5).
    assertEquals(1f, FgPolicy.snapVideoFitFloor(2f), 0f)
  }

  // ---------------------------------------------------------------- FIX-5: GPU class, budget, hybrid input

  @Test
  fun `gpu class is A7XX_PLUS only for Adreno 7xx and 8xx`() {
    val table = mapOf(
      "Adreno (TM) 740" to FgGpuClass.A7XX_PLUS,
      "Adreno (TM) 750" to FgGpuClass.A7XX_PLUS,
      "adreno 830" to FgGpuClass.A7XX_PLUS,
      "ADRENO (TM) 700" to FgGpuClass.A7XX_PLUS,
      "Adreno (TM) 899" to FgGpuClass.A7XX_PLUS,
      "Adreno (TM) 699" to FgGpuClass.OTHER,
      "Adreno (TM) 619" to FgGpuClass.OTHER,
      "Adreno (TM) 900" to FgGpuClass.OTHER,
      "Mali-G715" to FgGpuClass.OTHER,
      "PowerVR B-Series BXM-8-256" to FgGpuClass.OTHER,
      "Adreno" to FgGpuClass.OTHER,
      "" to FgGpuClass.OTHER,
    )
    for ((name, expected) in table) assertEquals(name, expected, FgPolicy.gpuClassOf(name))
    assertEquals(FgGpuClass.OTHER, FgPolicy.gpuClassOf(null))
  }

  /** LSFG compute of the cost model for an input of [w]×[h], in ms. */
  private fun compMs(w: Int, h: Int, n: Int, gpuFactor: Double = 1.0): Double =
    FgPolicy.COST_BASE_MS +
      w.toDouble() * h / 1e6 * gpuFactor * (FgPolicy.COST_FLOW_MS_PER_MP + FgPolicy.COST_GEN_MS_PER_MP * (n - 1))

  @Test
  fun `budget scale solves the cost model at half the period`() {
    // 24 fps: budget 20.83 ms; N = 5: 11.9 ms/MP → 1.4986 MP of 4.9056 → k = 0.5527.
    val k = FgPolicy.budgetInputScale(24.0, 5, 2800, 1752, true, FgGpuClass.A7XX_PLUS)
    assertEquals(0.5527, k, 1e-4)
    assertEquals(0.5 * 1000.0 / 24.0, compMs((2800 * k).toInt(), (1752 * k).toInt(), 5), 0.05)
    // N = 2: 4.7 ms/MP → k = 0.8795.
    assertEquals(0.8795, FgPolicy.budgetInputScale(24.0, 2, 2800, 1752, true, FgGpuClass.A7XX_PLUS), 1e-4)
    // OTHER: costs × 4 → k halves.
    assertEquals(k / 2, FgPolicy.budgetInputScale(24.0, 5, 2800, 1752, true, FgGpuClass.OTHER), 1e-9)
    // Quality mode: costs × 2 → k / √2.
    assertEquals(
      k / kotlin.math.sqrt(2.0),
      FgPolicy.budgetInputScale(24.0, 5, 2800, 1752, false, FgGpuClass.A7XX_PLUS),
      1e-9,
    )
    // More fps or a larger N → a smaller factor.
    assertTrue(FgPolicy.budgetInputScale(30.0, 5, 2800, 1752, true, FgGpuClass.A7XX_PLUS) < k)
    assertTrue(FgPolicy.budgetInputScale(24.0, 4, 2800, 1752, true, FgGpuClass.A7XX_PLUS) > k)
  }

  @Test
  fun `budget scale edges`() {
    val at30 = FgPolicy.budgetInputScale(30.0, 3, 2460, 1080, true, FgGpuClass.A7XX_PLUS)
    // Invalid fps → 30 fps.
    for (fps in listOf(0.0, -24.0, Double.NaN, Double.POSITIVE_INFINITY, null)) {
      assertEquals("fps=$fps", at30, FgPolicy.budgetInputScale(fps, 3, 2460, 1080, true, FgGpuClass.A7XX_PLUS), 0.0)
    }
    // N is clamped to 2..5.
    assertEquals(
      FgPolicy.budgetInputScale(24.0, 2, 2460, 1080, true, FgGpuClass.A7XX_PLUS),
      FgPolicy.budgetInputScale(24.0, 0, 2460, 1080, true, FgGpuClass.A7XX_PLUS),
      0.0,
    )
    assertEquals(
      FgPolicy.budgetInputScale(24.0, 5, 2460, 1080, true, FgGpuClass.A7XX_PLUS),
      FgPolicy.budgetInputScale(24.0, 9, 2460, 1080, true, FgGpuClass.A7XX_PLUS),
      0.0,
    )
    // 240 fps: budget 2.08 ms ≤ the 3 ms base → 0.
    assertEquals(0.0, FgPolicy.budgetInputScale(240.0, 2, 2460, 1080, true, FgGpuClass.A7XX_PLUS), 0.0)
    // Invalid output → 1 (no limit).
    assertEquals(1.0, FgPolicy.budgetInputScale(24.0, 5, 0, 1080, true, FgGpuClass.A7XX_PLUS), 0.0)
  }

  private fun hybrid(
    outW: Int = 2800,
    outH: Int = 1752,
    scale: Float = 1f,
    video: Pair<Int, Int>? = 1280 to 720,
    fps: Double? = 24.0,
    n: Int = 5,
    priority: FgPriority = FgPriority.FLUIDITY,
    gpu: FgGpuClass = FgGpuClass.A7XX_PLUS,
    screen: Boolean = false,
    quality: Boolean = true,
  ) = FgPolicy.inputSizeHybrid(outW, outH, scale, video, fps, n, priority, gpu, screen, quality)

  @Test
  fun `engine on 720p on the tablet sits between 1_2x the video and the budget`() {
    // kCover = 1280/2800 = 0.4571, 2·kCover = 0.9143, kBudget(5) = 0.5527 → 1548×968.
    val plan = hybrid()
    assertEquals(FgInputPlan(1548, 968, 5), plan)
    assertTrue(plan.width >= 1.2 * 1280 && plan.height >= 1.2 * 720)
    assertTrue(compMs(plan.width, plan.height, 5) <= 0.5 * 1000.0 / 24.0 + 0.05)
    // Close to the report's ≈ 1600×1000.
    assertTrue(plan.width in 1500..1650 && plan.height in 940..1030)
  }

  @Test
  fun `sharpness caps N at 2 and gives a larger input`() {
    val fluid = hybrid()
    val sharp = hybrid(priority = FgPriority.SHARPNESS)
    // kBudget(2) = 0.8795 < 2·kCover = 0.9143 → 2462×1540.
    assertEquals(FgInputPlan(2462, 1540, 2), sharp)
    assertTrue(sharp.effectiveN <= 2)
    assertTrue(sharp.width > fluid.width && sharp.height > fluid.height)
    // A requested 2× stays 2×; Fluidity keeps the requested N.
    assertEquals(2, hybrid(n = 2, priority = FgPriority.SHARPNESS).effectiveN)
    assertEquals(4, hybrid(n = 4).effectiveN)
    // Sharpness only bounds N while the quality engine drives the size.
    assertEquals(5, hybrid(priority = FgPriority.SHARPNESS, quality = false).effectiveN)
    assertEquals(5, hybrid(priority = FgPriority.SHARPNESS, screen = true).effectiveN)
  }

  @Test
  fun `engine on is capped at twice the video`() {
    // 30 fps, N = 2: kBudget = sqrt(13.67 / 4.7 / 4.9056) = 0.7700; 2·kCover(480p) = 2·854/2800 = 0.61.
    val plan = hybrid(video = 854 to 480, fps = 30.0, n = 2)
    assertEquals(FgInputPlan(1708, 1068, 2), plan)
  }

  @Test
  fun `screen resolution switch forces output times scale`() {
    assertEquals(FgInputPlan(2800, 1752, 5), hybrid(screen = true))
    assertEquals(FgInputPlan(2100, 1314, 5), hybrid(screen = true, scale = 0.75f))
    // Also with the engine off and with the strictest budget.
    assertEquals(FgInputPlan(2800, 1752, 5), hybrid(screen = true, quality = false))
    assertEquals(FgInputPlan(2460, 1080, 5), hybrid(outW = 2460, outH = 1080, screen = true, gpu = FgGpuClass.OTHER))
  }

  @Test
  fun `engine off never goes below the video`() {
    // Default floor 1.0: kOut = 0.4571 → 1280×800.
    val plan = hybrid(quality = false)
    assertEquals(FgInputPlan(1280, 800, 5), plan)
    assertTrue(plan.width >= 1280 && plan.height >= 720)
    // Same as the engine-off rule.
    assertEquals(FgPolicy.inputSize(2800, 1752, 1f, 1280 to 720), plan.width to plan.height)
    for ((ow, oh) in listOf(2800 to 1752, 2460 to 1080, 1080 to 2460, 1920 to 1080)) {
      for ((vw, vh) in listOf(1920 to 1080, 1280 to 720, 1080 to 1920, 640 to 480, 1440 to 1080)) {
        for (gpu in FgGpuClass.entries) {
          val p = hybrid(outW = ow, outH = oh, video = vw to vh, quality = false, gpu = gpu)
          val msg = "out=$ow×$oh video=$vw×$vh gpu=$gpu → ${p.width}×${p.height}"
          assertTrue(msg, p.width >= min(vw, ow) - 2 && p.height >= min(vh, oh) - 2)
        }
      }
    }
  }

  @Test
  fun `other gpu on the moto stays below the A7XX input and never below the video`() {
    // Moto 2460×1080, 720p at 24 fps, N = 5.
    val a7 = hybrid(outW = 2460, outH = 1080, gpu = FgGpuClass.A7XX_PLUS)
    val other = hybrid(outW = 2460, outH = 1080, gpu = FgGpuClass.OTHER)
    // A7XX: kBudget = 0.7510 → 1848×810; OTHER: kBudget = 0.3755 < kIn = 0.5203 → 1280×562.
    assertEquals(FgInputPlan(1848, 810, 5), a7)
    assertEquals(FgInputPlan(1280, 562, 5), other)
    assertTrue(other.width < a7.width && other.height < a7.height)
    // The budget never takes the input below the video that fits (kIn).
    assertEquals(FgPolicy.inputSize(2460, 1080, 1f, 1280 to 720, floor = 0.0), other.width to other.height)
  }

  @Test
  fun `hybrid edges with invalid fps and no video`() {
    // fps 0, NaN or null → 30 fps.
    val at30 = hybrid(fps = 30.0)
    for (fps in listOf(0.0, Double.NaN, null, -1.0)) assertEquals("fps=$fps", at30, hybrid(fps = fps))
    // No video with the engine on: k = min(s, max(kBudget, kMin)) → the budget alone.
    val kBudget = FgPolicy.budgetInputScale(24.0, 5, 2800, 1752, true, FgGpuClass.A7XX_PLUS)
    assertEquals(FgInputPlan(1548, 968, 5), hybrid(video = null))
    assertEquals(FgPolicy.inputSize(2800, 1752, kBudget.toFloat()), hybrid(video = null).let { it.width to it.height })
    // No video on OTHER: the budget is below kMin = 360/1752 → never below kMin.
    val small = hybrid(video = null, gpu = FgGpuClass.OTHER, fps = 60.0)
    assertTrue(small.height >= 360 - 2)
    // No video with the engine off → the three argument overload.
    assertEquals(FgInputPlan(2800, 1752, 5), hybrid(video = null, quality = false))
    // Invalid video sides count as no video.
    assertEquals(hybrid(video = null), hybrid(video = 0 to 720))
    // Invalid output → the three argument overload; N still clamped (and capped by Sharpness).
    assertEquals(FgInputPlan(2, 2, 2), hybrid(outW = 0, outH = -5, n = 1))
    assertEquals(FgInputPlan(2, 1080, 2), hybrid(outW = 0, outH = 1080, n = 9, priority = FgPriority.SHARPNESS))
    assertEquals(FgInputPlan(2, 1080, 5), hybrid(outW = 0, outH = 1080, n = 9))
  }

  @Test
  fun `hybrid properties even sides within the output and same aspect`() {
    val outputs = listOf(2800 to 1752, 2460 to 1080, 1080 to 2460, 1920 to 1080, 1280 to 800)
    val videos = listOf(1920 to 1080, 1280 to 720, 3840 to 2160, 1080 to 1920, 640 to 480, 2 to 2)
    for ((ow, oh) in outputs) for ((vw, vh) in videos) for (s in listOf(1f, 0.75f)) {
      for (gpu in FgGpuClass.entries) for (n in 2..5) for (prio in FgPriority.entries) {
        for (quality in listOf(false, true)) for (screen in listOf(false, true)) {
          val p = hybrid(ow, oh, s, vw to vh, 24.0, n, prio, gpu, screen, quality)
          val msg = "out=$ow×$oh video=$vw×$vh s=$s gpu=$gpu n=$n $prio q=$quality scr=$screen → $p"
          assertEquals(msg, 0, p.width % 2)
          assertEquals(msg, 0, p.height % 2)
          assertTrue(msg, p.width in 2..ow && p.height in 2..oh)
          assertTrue(msg, p.effectiveN in 2..n)
          val (inLong, inShort, outLong, outShort) =
            if (ow >= oh) listOf(p.width, p.height, ow, oh) else listOf(p.height, p.width, oh, ow)
          assertTrue(msg, kotlin.math.abs(inShort - inLong.toDouble() * outShort / outLong) <= 2.0)
        }
      }
    }
  }

  // ---------------------------------------------------------------- shouldUpdateLatency

  @Test
  fun `latency update needs 8 ms quantized delta and 2 s interval`() {
    // 41.9 → 40 (quantum 4): |40 − 32| = 8 → update.
    assertTrue(FgPolicy.shouldUpdateLatency(32.0, 41.9, 2000))
    // 37.9 → 36: |36 − 32| = 4 → no.
    assertFalse(FgPolicy.shouldUpdateLatency(32.0, 37.9, 5000))
    // 38.1 → 40: delta 8 → yes; but only 1999 ms elapsed → no.
    assertTrue(FgPolicy.shouldUpdateLatency(32.0, 38.1, 2000))
    assertFalse(FgPolicy.shouldUpdateLatency(32.0, 38.1, 1999))
    // Decreasing latency also counts.
    assertTrue(FgPolicy.shouldUpdateLatency(40.0, 31.0, 10_000)) // 31 → 32
    assertFalse(FgPolicy.shouldUpdateLatency(40.0, 35.0, 10_000)) // 35 → 36: delta 4
    // Non-finite measurements never trigger an update.
    assertFalse(FgPolicy.shouldUpdateLatency(32.0, Double.NaN, 10_000))
  }
}
