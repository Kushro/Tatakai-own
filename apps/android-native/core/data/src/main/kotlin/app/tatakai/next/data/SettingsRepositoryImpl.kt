package app.tatakai.next.data

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.booleanPreferencesKey
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.stringPreferencesKey
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.runBlocking

/**
 * DataStore-backed settings. The first value is read synchronously at construction (a few ms on a cold start) so the
 * theme never flashes the defaults before the stored palette arrives.
 */
class SettingsRepositoryImpl(
    private val store: DataStore<Preferences>,
    scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) : SettingsRepository {

    override val settings: StateFlow<AppSettings> = run {
        val initial = runBlocking(Dispatchers.IO) { store.data.first().toSettings() }
        store.data.map { it.toSettings() }.stateIn(scope, SharingStarted.Eagerly, initial)
    }

    override suspend fun update(transform: (AppSettings) -> AppSettings) {
        store.edit { prefs ->
            val next = transform(prefs.toSettings())
            prefs[PALETTE] = next.paletteId
            prefs[AMOLED] = next.amoled
            prefs[CONTRAST] = next.contrast.name
            prefs[DYNAMIC] = next.dynamicColor
            prefs[PRIORITY] = next.providerPriority.distinct().joinToString(",")
            prefs[HEADERS_DEBUG] = next.playerHeadersDebug
        }
    }

    private fun Preferences.toSettings(): AppSettings {
        val d = AppSettings()
        return AppSettings(
            paletteId = this[PALETTE] ?: d.paletteId,
            amoled = this[AMOLED] ?: d.amoled,
            contrast = this[CONTRAST]?.let { n -> ContrastPref.entries.firstOrNull { it.name == n } } ?: d.contrast,
            dynamicColor = this[DYNAMIC] ?: d.dynamicColor,
            providerPriority = this[PRIORITY]?.split(',')?.filter { it.isNotBlank() } ?: d.providerPriority,
            playerHeadersDebug = this[HEADERS_DEBUG] ?: d.playerHeadersDebug,
        )
    }

    private companion object {
        val PALETTE = stringPreferencesKey("palette_id")
        val AMOLED = booleanPreferencesKey("amoled")
        val CONTRAST = stringPreferencesKey("contrast")
        val DYNAMIC = booleanPreferencesKey("dynamic_color")
        val PRIORITY = stringPreferencesKey("provider_priority")
        val HEADERS_DEBUG = booleanPreferencesKey("player_headers_debug")
    }
}
