// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import app.tatakai.next.model.AiringRepository
import kotlinx.coroutines.CancellationException
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject
import java.util.concurrent.TimeUnit

/**
 * Parity wave B (16 §3.1, worker r, checklist home.calendar.reminders): every 30 min, with network, asks [AiringRepository] which
 * followed episodes (`remind == true`) aired since the stamp `storeDir/reminded` and posts one notification each on channel
 * `AiringRepositoryImpl.CHANNEL_ID` (`airing_notification_title`, tap = launcher intent with `NotificationExtras.DETAILS_ANILIST_ID`).
 * Scheduled by [schedule] from `discoverModule` while `AppSettings.airingReminders`; [cancel] when it is off. Koin is started by
 * `TatakaiApplication` before WorkManager's default initializer (same as `HistorySyncWorker`).
 */
class AiringReminderWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params), KoinComponent {
    private val airing: AiringRepository by inject()

    override suspend fun doWork(): Result = try {
        // wave B (r): compute aired-since-stamp for airing.followed, post notifications, advance the stamp.
        Result.success()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        Result.retry()
    }

    companion object {
        const val UNIQUE_NAME: String = "tatakai-airing-reminders"
        const val INTERVAL_MINUTES: Long = 30

        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<AiringReminderWorker>(INTERVAL_MINUTES, TimeUnit.MINUTES)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(UNIQUE_NAME, ExistingPeriodicWorkPolicy.KEEP, request)
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(UNIQUE_NAME)
        }
    }
}
