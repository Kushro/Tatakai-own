// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.core.edit
import androidx.datastore.preferences.core.longPreferencesKey
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/**
 * Epoch ms of the last successful history sync (`HistoryRepository.lastSyncedAt`, 12 §3.6). Persisted in the settings DataStore under
 * its own key (SettingsCodec ignores unknown keys); [store] null = in memory only (tests). null = never / signed out.
 */
class HistorySyncStamp(
    private val store: DataStore<Preferences>? = null,
    scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO),
) {
    private val _value = MutableStateFlow<Long?>(null)
    val value: StateFlow<Long?> = _value.asStateFlow()

    init {
        if (store != null) {
            scope.launch {
                val stored = runCatching { store.data.first()[KEY] }.getOrNull()
                // a sync that finished while loading wins
                if (_value.value == null) _value.value = stored
            }
        }
    }

    suspend fun set(ms: Long?) {
        _value.value = ms
        store?.let { s ->
            runCatching { s.edit { p -> if (ms == null) p.remove(KEY) else p[KEY] = ms } }
        }
    }

    private companion object {
        val KEY = longPreferencesKey("history_last_synced_ms")
    }
}
