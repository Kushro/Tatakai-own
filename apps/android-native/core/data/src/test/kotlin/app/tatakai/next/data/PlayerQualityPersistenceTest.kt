// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import androidx.datastore.preferences.core.emptyPreferences
import androidx.datastore.preferences.core.mutablePreferencesOf
import androidx.datastore.preferences.core.stringPreferencesKey
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.DebandConfig
import app.tatakai.next.model.DecoderMode
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.PlayerQualityPrefs
import app.tatakai.next.model.QualityPresets
import app.tatakai.next.model.QualityTier
import app.tatakai.next.model.SharpenLevel
import org.junit.Assert.assertEquals
import org.junit.Test

/** 0.4.0 (worker A): engine default, desktop preset ids and the quality-engine defaults. */
class PlayerQualityPersistenceTest {
    @Test
    fun newInstallDefaultsToMpv() {
        assertEquals(PlayerEngine.MPV, SettingsCodec.read(emptyPreferences()).playerEngine)
    }

    @Test
    fun storedMedia3IsKept() {
        // Every write stores every key, so a stored MEDIA3 may be an explicit choice: it is not migrated.
        val p = mutablePreferencesOf(stringPreferencesKey("player_engine") to "MEDIA3")
        assertEquals(PlayerEngine.MEDIA3, SettingsCodec.read(p).playerEngine)
    }

    @Test
    fun everyDesktopPresetIdRoundTrips() {
        QualityPresets.ALL.forEach { id ->
            val p = mutablePreferencesOf()
            SettingsCodec.write(p, AppSettings(playerPreset = id))
            assertEquals(id, SettingsCodec.read(p).playerPreset)
        }
    }

    @Test
    fun qualityPrefsRoundTripSanitized() {
        val prefs = PlayerQualityPrefs(
            tier = QualityTier.HQ,
            deband = DebandConfig(enabled = false, iterations = 99, threshold = 50, range = 0, grain = 10),
            autoAdjust = false,
            decoder = DecoderMode.SW,
            smoothing = true,
            sharpen = SharpenLevel.HIGH,
        )
        val p = mutablePreferencesOf()
        SettingsCodec.write(p, AppSettings(playerQuality = prefs))
        val read = SettingsCodec.read(p).playerQuality
        assertEquals(prefs.copy(deband = prefs.deband.copy(iterations = 16, range = 1)), read)
    }

    @Test
    fun brokenQualityJsonReadsDefaults() {
        val p = mutablePreferencesOf(stringPreferencesKey("player_quality_json") to "{\"tier\":\"ultra\"")
        assertEquals(PlayerQualityPrefs(), SettingsCodec.read(p).playerQuality)
    }
}
