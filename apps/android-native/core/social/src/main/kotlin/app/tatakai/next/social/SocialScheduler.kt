// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import android.content.Context
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * Parity wave B (16 §5 t.1, worker t): keeps [SocialNotificationsWorker] scheduled exactly while `AppSettings.socialNotifications` is on
 * AND an AniList account is linked (observes `ListsRepository.accounts`), and cancels it otherwise. Created eagerly by `socialModule`.
 */
class SocialScheduler(
    private val context: Context,
    settings: SettingsRepository,
    lists: ListsRepository,
    private val scope: CoroutineScope = CoroutineScope(SupervisorJob() + Dispatchers.Default),
) {
    init {
        scope.launch {
            wanted(settings, lists).collect { on ->
                runCatching {
                    if (on) SocialNotificationsWorker.schedule(context) else SocialNotificationsWorker.cancel(context)
                }
            }
        }
    }

    companion object {
        /** True while polling must run. */
        fun wanted(settings: SettingsRepository, lists: ListsRepository): Flow<Boolean> =
            combine(
                settings.settings.map { it.socialNotifications },
                lists.accounts.map { list -> list.any { it.provider == ListProvider.ANILIST } },
            ) { enabled, linked -> enabled && linked }.distinctUntilChanged()
    }
}
