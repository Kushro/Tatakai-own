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

/**
 * Pull cursor of the Link history sync (`next_since_unix` of the last page, unix seconds), persisted in the settings DataStore under
 * `history_link_since_unix` (SettingsCodec ignores unknown keys). [value] is read synchronously by `HistorySyncer`; until the stored
 * value is loaded it is 0, which only means a full (idempotent) pull. [store] null = in memory only (tests).
 */
class HistoryCursorStore(
    private val store: DataStore<Preferences>? = null,
    scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO),
) {
    @Volatile private var current: Long = 0L
    @Volatile private var touched = false

    init {
        if (store != null) {
            scope.launch {
                val stored = runCatching { store.data.first()[KEY] }.getOrNull()
                if (!touched && stored != null) current = stored
            }
        }
    }

    fun value(): Long = current

    suspend fun set(sinceUnix: Long) {
        touched = true
        current = sinceUnix.coerceAtLeast(0L)
        store?.let { s -> runCatching { s.edit { p -> p[KEY] = current } } }
    }

    private companion object {
        val KEY = longPreferencesKey("history_link_since_unix")
    }
}

/**
 * When the history syncs over Tatakai Link besides the player's `HistoryFlush`: once at app open, and whenever a PC gets paired (the
 * first pull brings its history down). Forgetting the PC resets the stamp and the cursor (they belonged to that pairing).
 */
class HistorySyncTriggers(
    private val scheduler: HistorySyncScheduler,
    private val link: app.tatakai.next.model.LinkRepository?,
    private val syncer: HistorySyncer?,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.IO),
) {
    fun start() {
        scheduler.requestSync(immediate = true)
        val repo = link ?: return
        scope.launch {
            var wasPaired: Boolean? = null
            repo.state.collect { state ->
                val paired = state is app.tatakai.next.model.LinkState.Paired
                val unpaired = state is app.tatakai.next.model.LinkState.Unpaired
                if (wasPaired == false && paired) scheduler.requestSync(immediate = true)
                if (wasPaired == true && unpaired) runCatching { syncer?.resetStamp() }
                if (paired || unpaired) wasPaired = paired
            }
        }
    }
}
