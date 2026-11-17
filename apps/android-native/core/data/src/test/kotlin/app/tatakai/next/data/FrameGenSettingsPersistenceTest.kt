// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import androidx.datastore.preferences.core.emptyPreferences
import androidx.datastore.preferences.core.mutablePreferencesOf
import androidx.datastore.preferences.core.stringPreferencesKey
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.FrameGenPriority
import app.tatakai.next.model.FrameGenProfile
import app.tatakai.next.model.FrameGenSettings
import org.junit.Assert.assertEquals
import org.junit.Test

/** 0.4.0 (worker B): `AppSettings.frameGen` round-trips through SettingsCodec (`frame_gen_json`) and is sanitized on read. */
class FrameGenSettingsPersistenceTest {
    @Test
    fun `missing key reads as the defaults`() {
        assertEquals(FrameGenSettings(), SettingsCodec.read(emptyPreferences()).frameGen)
    }

    @Test
    fun `custom values round-trip`() {
        val fg = FrameGenSettings(
            enabled = true,
            multiplier = 4,
            profile = FrameGenProfile.QUALITY,
            customPerformance = false,
            customFlowScale = 0.75f,
            customInputScale = 0.75f,
            matchRefresh = false,
            audioOffsetMs = -40,
            allowUntestedVulkan = true,
            showStats = true,
            videoFitFloor = 0.5f,
            priority = FrameGenPriority.SHARPNESS,
            screenResolution = true,
        )
        val prefs = mutablePreferencesOf()
        SettingsCodec.write(prefs, AppSettings(frameGen = fg))
        assertEquals(fg, SettingsCodec.read(prefs).frameGen)
    }

    @Test
    fun `out of range values are sanitized and garbage reads as defaults`() {
        val prefs = mutablePreferencesOf(
            stringPreferencesKey("frame_gen_json") to
                """{"enabled":true,"multiplier":9,"customFlowScale":7,"audioOffsetMs":9000,"videoFitFloor":0.6,"unknown":1}""",
        )
        val fg = SettingsCodec.read(prefs).frameGen
        assertEquals(true, fg.enabled)
        assertEquals(FrameGenSettings.MULTIPLIER_AUTO, fg.multiplier)
        assertEquals(1f, fg.customFlowScale)
        assertEquals(500, fg.audioOffsetMs)
        assertEquals(0.5f, fg.videoFitFloor)

        prefs[stringPreferencesKey("frame_gen_json")] = "not json"
        assertEquals(FrameGenSettings(), SettingsCodec.read(prefs).frameGen)
    }
}
