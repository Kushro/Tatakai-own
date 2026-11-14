// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import app.tatakai.next.player.ShaderFallback

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class PresetGovernorTest {
    @Test
    fun lowBatteryKeepsAnime4KOffUnlessCharging() {
        // BUG-006: the setting promises Anime4K off below 20 %.
        assertEquals("off" to ShaderFallback.OFF_BATTERY, PresetGovernor("anime4k-aa").initial(BatteryInfo(12, charging = false)))
        assertEquals("anime4k-aa" to null, PresetGovernor("anime4k-aa").initial(BatteryInfo(12, charging = true)))
        assertEquals("anime4k-a" to null, PresetGovernor("anime4k-a").initial(BatteryInfo(20, charging = false)))
        assertEquals("fsr" to null, PresetGovernor("fsr").initial(BatteryInfo(5, charging = false)), "FSR is not Anime4K")
        assertEquals("anime4k-b" to null, PresetGovernor("anime4k-b").initial(null))
        val g = PresetGovernor("anime4k-b")
        assertNull(g.onBattery(BatteryInfo(40, false)))
        assertEquals(ShaderFallback.OFF_BATTERY, g.onBattery(BatteryInfo(19, false)))
        assertEquals("off", g.preset)
    }

    @Test
    fun aChainThatDropsFramesStepsDownToLiteThenOff() {
        // BUG-043: 7 fps presented of 24 = ~70 % dropped.
        val g = PresetGovernor("anime4k-a")
        var t = 0L
        var drops = 0L
        fun sample(dropPerSec: Int): ShaderFallback? {
            t += 2_000
            drops += dropPerSec * 2
            return g.onSample(drops, 23.976, t)
        }
        assertNull(sample(17)) // first sample: baseline
        assertNull(sample(17)) // 1 slow window
        assertEquals(ShaderFallback.LIGHTER, sample(17))
        assertEquals("anime4k-a-lite", g.preset)
        assertNull(sample(17)) // new baseline
        assertNull(sample(17))
        assertEquals(ShaderFallback.OFF_SLOW, sample(17))
        assertEquals("off", g.preset)
        assertNull(sample(17), "nothing below off")
    }

    @Test
    fun aRealTimeChainStaysAndASingleHiccupIsForgiven() {
        val g = PresetGovernor("anime4k-c")
        var t = 0L
        var drops = 0L
        listOf(0, 0, 20, 0, 1, 0, 20, 0).forEach { perSec ->
            t += 2_000
            drops += perSec * 2
            assertNull(g.onSample(drops, 24.0, t))
        }
        assertEquals("anime4k-c", g.preset)
    }

    @Test
    fun lighterSteps() {
        assertEquals("anime4k-aa-lite", ShaderPresets.lighter("anime4k-aa"))
        assertEquals("off", ShaderPresets.lighter("anime4k-aa-lite"))
        assertEquals("off", ShaderPresets.lighter("fsr"))
        assertNull(ShaderPresets.lighter("off"))
        assertEquals(listOf("Anime4K_Clamp_Highlights.glsl", "Anime4K_Upscale_CNN_x2_S.glsl"), ShaderPresets.chain("anime4k-c-lite"))
    }
}
