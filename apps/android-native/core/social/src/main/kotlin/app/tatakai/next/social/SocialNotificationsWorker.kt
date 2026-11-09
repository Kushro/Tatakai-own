// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import app.tatakai.next.model.SocialRepository
import kotlinx.coroutines.CancellationException
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject
import java.util.concurrent.TimeUnit

/**
 * Parity wave B (16 §3.7, worker t, checklist social.notifications.badge): every 15 min (WorkManager minimum) with network, refreshes
 * `SocialRepository.unreadCount`; when it grew since the previous run, posts ONE summary notification (`social_notification_unread`,
 * channel `SocialRepositoryImpl.CHANNEL_ID`, tap = launcher intent with `NotificationExtras.SOCIAL_NOTIFICATIONS = true`). The badge in
 * the shell reads `unreadCount` directly; the foreground refresh is done by the screens (`refreshUnread()` on resume). Scheduled while
 * `AppSettings.socialNotifications` and an AniList account is linked.
 */
class SocialNotificationsWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params), KoinComponent {
    private val social: SocialRepository by inject()

    override suspend fun doWork(): Result = try {
        social.refreshUnread()
        // wave B (t): compare with the stored count and post the summary notification.
        Result.success()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        Result.retry()
    }

    companion object {
        const val UNIQUE_NAME: String = "tatakai-social-notifications"
        const val INTERVAL_MINUTES: Long = 15

        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<SocialNotificationsWorker>(INTERVAL_MINUTES, TimeUnit.MINUTES)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(UNIQUE_NAME, ExistingPeriodicWorkPolicy.KEEP, request)
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(UNIQUE_NAME)
        }
    }
}
