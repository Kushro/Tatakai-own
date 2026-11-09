// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import android.content.Context
import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * Parity wave B (16 §3.1, worker r): app-lifetime reaction to `AppSettings.airingReminders` — schedules [AiringReminderWorker] while on,
 * cancels it when off. Created eagerly by `discoverModule` (`createdAtStart`), so it runs from app start.
 */
class DiscoverScheduler(private val context: Context, settings: SettingsRepository) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    init {
        scope.launch {
            settings.settings.map { it.airingReminders }.distinctUntilChanged().collect { on ->
                runCatching { if (on) AiringReminderWorker.schedule(context) else AiringReminderWorker.cancel(context) }
            }
        }
    }
}
