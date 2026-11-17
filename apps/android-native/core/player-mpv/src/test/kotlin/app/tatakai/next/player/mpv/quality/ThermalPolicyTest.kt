// SPDX-License-Identifier: AGPL-3.0-or-later
// Ported to Tatakai 0.4.0 (worker A) from Enigmata `app/src/test/java/app/enigmata/domain/quality/ThermalPolicyTest.kt` (AGPL-3.0-or-later, same author): package renamed, re-indented.
package app.tatakai.next.player.mpv.quality

import org.junit.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/**
 * [ThermalPolicy.isDegraded]: entry by status (>= SEVERE) and by headroom (>= 0.95, where 1.0 is
 * the SEVERE threshold), NaN/null as "no data", hysteresis between the entry and exit thresholds,
 * and exit only below MODERATE with headroom < 0.80 or unknown.
 */
class ThermalPolicyTest {

    private val none = 0
    private val light = 1
    private val moderate = 2
    private val severe = 3
    private val critical = 4

    @Test
    fun `platform status constants match PowerManager values`() {
        assertEquals(2, ThermalPolicy.STATUS_MODERATE)
        assertEquals(3, ThermalPolicy.STATUS_SEVERE)
    }

    @Test
    fun `no data at all is never degraded`() {
        assertFalse(ThermalPolicy.isDegraded(null, null, wasDegraded = false))
        assertFalse(ThermalPolicy.isDegraded(null, Float.NaN, wasDegraded = false))
    }

    @Test
    fun `NaN headroom is ignored and does not trigger entry`() {
        assertFalse(ThermalPolicy.isDegraded(none, Float.NaN, wasDegraded = false))
        assertFalse(ThermalPolicy.isDegraded(moderate, Float.NaN, wasDegraded = false))
        assertTrue(ThermalPolicy.isDegraded(severe, Float.NaN, wasDegraded = false))
    }

    @Test
    fun `NaN headroom does not block exit when status is cool`() {
        assertFalse(ThermalPolicy.isDegraded(light, Float.NaN, wasDegraded = true))
    }

    @Test
    fun `null headroom behaves like NaN`() {
        assertFalse(ThermalPolicy.isDegraded(moderate, null, wasDegraded = false))
        assertTrue(ThermalPolicy.isDegraded(moderate, null, wasDegraded = true))
        assertFalse(ThermalPolicy.isDegraded(none, null, wasDegraded = true))
    }

    @Test
    fun `enters by status at SEVERE and above`() {
        assertTrue(ThermalPolicy.isDegraded(severe, 0.1f, wasDegraded = false))
        assertTrue(ThermalPolicy.isDegraded(critical, null, wasDegraded = false))
        assertFalse(ThermalPolicy.isDegraded(moderate, 0.1f, wasDegraded = false))
    }

    @Test
    fun `enters by headroom at 0_95 and above even with a cool status`() {
        assertTrue(ThermalPolicy.isDegraded(none, 0.95f, wasDegraded = false))
        assertTrue(ThermalPolicy.isDegraded(null, 1.2f, wasDegraded = false))
        assertFalse(ThermalPolicy.isDegraded(none, 0.94f, wasDegraded = false))
    }

    @Test
    fun `hysteresis band keeps the previous state`() {
        // headroom 0.85 sits between exit (0.80) and entry (0.95).
        assertTrue(ThermalPolicy.isDegraded(none, 0.85f, wasDegraded = true))
        assertFalse(ThermalPolicy.isDegraded(none, 0.85f, wasDegraded = false))
        // MODERATE status sits between exit (< MODERATE) and entry (SEVERE).
        assertTrue(ThermalPolicy.isDegraded(moderate, 0.5f, wasDegraded = true))
        assertFalse(ThermalPolicy.isDegraded(moderate, 0.5f, wasDegraded = false))
        // Exactly at the exit threshold is still inside the band.
        assertTrue(ThermalPolicy.isDegraded(none, 0.80f, wasDegraded = true))
    }

    @Test
    fun `exits only when status is below MODERATE and headroom below 0_80`() {
        assertFalse(ThermalPolicy.isDegraded(light, 0.79f, wasDegraded = true))
        assertFalse(ThermalPolicy.isDegraded(null, 0.5f, wasDegraded = true))
        assertTrue(ThermalPolicy.isDegraded(light, 0.85f, wasDegraded = true))
        assertTrue(ThermalPolicy.isDegraded(moderate, 0.1f, wasDegraded = true))
    }

    @Test
    fun `cooling sequence walks through the band before releasing`() {
        var degraded = false
        val samples = listOf(
            none to 0.70f to false,
            light to 0.90f to false,
            moderate to 0.96f to true,
            moderate to 0.85f to true,
            light to 0.82f to true,
            light to 0.79f to false,
            none to 0.90f to false,
        )
        for ((input, expected) in samples) {
            degraded = ThermalPolicy.isDegraded(input.first, input.second, degraded)
            assertEquals(expected, degraded, "status=${input.first} headroom=${input.second}")
        }
    }
}
