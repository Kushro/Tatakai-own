// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.emptyPreferences
import androidx.datastore.preferences.core.stringPreferencesKey
import app.tatakai.next.model.AppLanguage
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.model.ThemeMode
import app.tatakai.next.model.torrent.AfterWatch
import app.tatakai.next.model.torrent.StorageLocation
import app.tatakai.next.model.torrent.TorrentSettings
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

/** DataStore persistence of every AppSettings field (12 §4 a.4), on the JVM with an in-memory store. */
class SettingsPersistenceTest {
    private class MemoryStore(initial: Preferences = emptyPreferences()) : DataStore<Preferences> {
        val state = MutableStateFlow(initial)
        override val data: Flow<Preferences> = state

        override suspend fun updateData(transform: suspend (t: Preferences) -> Preferences): Preferences {
            val next = transform(state.value)
            state.value = next
            return next
        }
    }

    private val custom = AppSettings(
        paletteId = "sakura",
        listsWriteBack = false,
        amoled = true,
        contrast = ContrastPref.HIGH,
        dynamicColor = false,
        providerPriority = listOf("kitsu", "anilist"),
        playerHeadersDebug = true,
        themeMode = ThemeMode.DARK,
        reduceMotion = true,
        language = AppLanguage.ES,
        playerEngine = PlayerEngine.MPV,
        playerPreset = "anime4k-b",
        sourcePrefs = SourcePrefs(mode = SourceMode.MANUAL, prefer = SourceKind.TORRENT, minQuality = Quality.P1080, subLang = "en", dubLang = "es"),
        autoNext = false,
        skipIntro = false,
        subtitleStyle = SubtitleStyle(scale = 1.4f, backgroundOpacity = 0.2f, font = "space_grotesk", bold = true, outline = false),
        torrent = TorrentSettings(
            downloadLimitKib = 2048,
            uploadLimitKib = 64,
            listenPort = 51413,
            upnp = true,
            maxPeers = 80,
            dht = false,
            afterWatch = AfterWatch.KEEP,
            backgroundDownloads = true,
            backgroundAck = true,
            wifiOnly = false,
            pauseOnLowBattery = false,
            uploadOnMobileData = true,
            storageLocation = StorageLocation.UserFolder("content://com.android.externalstorage.documents/tree/primary%3AAnime"),
        ),
        metadataCacheTtlHours = 72,
        externalPlayerPackage = "is.xyz.mpv",
    )

    @Test
    fun emptyStoreReadsDefaults() {
        assertEquals(AppSettings(), SettingsCodec.read(emptyPreferences()))
    }

    @Test
    fun everyFieldRoundTrips() = runTest {
        val store = MemoryStore()
        store.edit { SettingsCodec.write(it, custom) }
        assertEquals(custom, SettingsCodec.read(store.state.value))
    }

    @Test
    fun repositoryUpdateAndConvenienceSettersPersist() = runTest {
        val store = MemoryStore()
        val repo = SettingsRepositoryImpl(store, backgroundScope)
        repo.update { custom }
        repo.setPlayerEngine(PlayerEngine.EXTERNAL)
        repo.setTorrentSettings(TorrentSettings(maxPeers = 9999, listenPort = 80, backgroundDownloads = true))
        repo.acknowledgeBackgroundDownloads()
        val read = SettingsCodec.read(store.state.value)
        assertEquals(PlayerEngine.EXTERNAL, read.playerEngine)
        assertEquals(TorrentSettings.MAX_PEERS, read.torrent.maxPeers)
        assertEquals(0, read.torrent.listenPort)
        // sanitized before the ack: background stays off until the user turns it on again
        assertFalse(read.torrent.backgroundDownloads)
        assertEquals(true, read.torrent.backgroundAck)
        assertEquals("sakura", read.paletteId)
    }

    @Test
    fun externalPlayerNullRemovesTheKey() = runTest {
        val store = MemoryStore()
        store.edit { SettingsCodec.write(it, custom) }
        store.edit { SettingsCodec.write(it, custom.copy(externalPlayerPackage = null)) }
        assertNull(SettingsCodec.read(store.state.value).externalPlayerPackage)
        assertNull(store.state.value[stringPreferencesKey("external_player_package")])
    }

    @Test
    fun legacyKeysSurviveAndBadValuesFallBackToDefaults() = runTest {
        val store = MemoryStore()
        store.edit {
            it[stringPreferencesKey("palette_id")] = "legacy"
            it[booleanPreferencesKey("amoled")] = true
            it[stringPreferencesKey("theme_mode")] = "PURPLE"
            it[stringPreferencesKey("torrent_settings_json")] = "{not json"
            it[stringPreferencesKey("source_prefs_json")] = """{"mode":"manual","future":1}"""
            it[stringPreferencesKey("player_preset")] = "unknown-preset"
        }
        val read = SettingsCodec.read(store.state.value)
        assertEquals("legacy", read.paletteId)
        assertEquals(true, read.amoled)
        assertEquals(ThemeMode.SYSTEM, read.themeMode)
        assertEquals(TorrentSettings(), read.torrent)
        assertEquals(SourcePrefs(mode = SourceMode.MANUAL), read.sourcePrefs)
        assertEquals("off", read.playerPreset)
    }

    @Test
    fun rangesAreClamped() {
        val store = MemoryStore()
        kotlinx.coroutines.runBlocking {
            store.edit { SettingsCodec.write(it, AppSettings(metadataCacheTtlHours = 9000, subtitleStyle = SubtitleStyle(scale = 9f, font = "comic"))) }
        }
        val read = SettingsCodec.read(store.state.value)
        assertEquals(168, read.metadataCacheTtlHours)
        assertEquals(2f, read.subtitleStyle.scale)
        assertEquals("outfit", read.subtitleStyle.font)
    }
}
