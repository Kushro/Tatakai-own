// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.DecoderMode
import app.tatakai.next.model.ImageAdjust
import app.tatakai.next.model.PlayerQualityPrefs
import app.tatakai.next.model.QualityTier
import app.tatakai.next.model.SharpenLevel
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// 0.4.0 (worker A): the shared quality types (both engines) and the "Cambiar a MPV" rule.
class QualityModelsTest {
    @Test
    fun requestFromSettings() {
        val prefs = PlayerQualityPrefs(tier = QualityTier.HQ, decoder = DecoderMode.HW_PLUS, smoothing = true, sharpen = SharpenLevel.LOW)
        val r = QualityRequest.from("anime4k-bb", prefs)
        assertEquals("anime4k-bb", r.preset)
        assertEquals(QualityTier.HQ, r.tier)
        assertEquals(DecoderMode.HW_PLUS, r.decoder)
        assertTrue(r.smoothing)
        assertEquals(SharpenLevel.LOW, r.sharpen)
        assertEquals("off", QualityRequest.from("nope", prefs).preset)
    }

    @Test
    fun sanitizeClampsLiveValues() {
        val r = QualityRequest(zoomPercent = 9000, image = ImageAdjust(hue = -500)).sanitized()
        assertEquals(QualityRequest.MAX_ZOOM, r.zoomPercent)
        assertEquals(-100, r.image.hue)
    }

    @Test
    fun levelsDropTheTierOfUntieredPresets() {
        assertEquals(QualityLevel("fsr", null), QualityLevel.of("fsr", QualityTier.HQ))
        assertEquals(QualityLevel.OFF, QualityLevel.of("off", QualityTier.HQ))
        assertEquals(QualityTier.FAST, QualityLevel.of("anime4k-a", QualityTier.FAST).tier)
        assertNull(QualityTier.FAST.lower())
        assertEquals(QualityTier.BALANCED, QualityTier.HQ.lower())
    }

    @Test
    fun media3PublishesNothingAvailable() {
        val s = QualityUiState.UNAVAILABLE.value
        assertFalse(s.available)
        assertTrue(s.presets.none { it.available })
        assertTrue(s.effective.off)
    }

    @Test
    fun decoderLabels() {
        assertEquals("HW+", decoderInUseLabel("mediacodec"))
        assertEquals("HW", decoderInUseLabel("mediacodec-copy"))
        assertEquals("SW", decoderInUseLabel("no"))
        assertNull(decoderInUseLabel(null))
    }

    @Test
    fun switchToMpvOnlyFromMedia3OutsideLink() {
        assertTrue(canSwitchToMpv(onMedia3 = true, mpvAvailable = true, remote = false))
        assertFalse(canSwitchToMpv(onMedia3 = false, mpvAvailable = true, remote = false))
        assertFalse(canSwitchToMpv(onMedia3 = true, mpvAvailable = false, remote = false))
        assertFalse(canSwitchToMpv(onMedia3 = true, mpvAvailable = true, remote = true))
    }
}
