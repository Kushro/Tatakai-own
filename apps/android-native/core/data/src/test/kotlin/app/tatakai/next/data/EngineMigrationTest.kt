package app.tatakai.next.data

import androidx.datastore.preferences.core.emptyPreferences
import androidx.datastore.preferences.core.mutablePreferencesOf
import androidx.datastore.preferences.core.stringPreferencesKey
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.PlayerEngine
import kotlin.test.Test
import kotlin.test.assertEquals

/** 0.4.0: libmpv + Anime4K A+A by default; <= 0.3.x installs on Media3 move once, a later explicit choice sticks. */
class EngineMigrationTest {
    private val engine = stringPreferencesKey("player_engine")
    private val preset = stringPreferencesKey("player_preset")

    @Test
    fun `new install uses libmpv with Anime4K A+A`() {
        val s = SettingsCodec.read(emptyPreferences())
        assertEquals(PlayerEngine.MPV, s.playerEngine)
        assertEquals("anime4k-aa", s.playerPreset)
    }

    @Test
    fun `0_3 install on Media3 with preset off moves to libmpv and A+A`() {
        val s = SettingsCodec.read(mutablePreferencesOf(engine to "MEDIA3", preset to "off"))
        assertEquals(PlayerEngine.MPV, s.playerEngine)
        assertEquals("anime4k-aa", s.playerPreset)
    }

    @Test
    fun `0_3 install keeps a preset it chose and an External engine`() {
        assertEquals("fsr", SettingsCodec.read(mutablePreferencesOf(engine to "MEDIA3", preset to "fsr")).playerPreset)
        assertEquals(PlayerEngine.EXTERNAL, SettingsCodec.read(mutablePreferencesOf(engine to "EXTERNAL")).playerEngine)
        assertEquals("off", SettingsCodec.read(mutablePreferencesOf(engine to "MPV", preset to "off")).playerPreset)
    }

    @Test
    fun `an explicit Media3 choice written by 0_4 sticks`() {
        val prefs = mutablePreferencesOf()
        SettingsCodec.write(prefs, AppSettings(playerEngine = PlayerEngine.MEDIA3, playerPreset = "off"))
        val s = SettingsCodec.read(prefs)
        assertEquals(PlayerEngine.MEDIA3, s.playerEngine)
        assertEquals("off", s.playerPreset)
    }
}
