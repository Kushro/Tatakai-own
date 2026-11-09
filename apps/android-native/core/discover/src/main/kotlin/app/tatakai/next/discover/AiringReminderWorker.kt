// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import android.Manifest
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationChannelCompat
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.NotificationExtras
import kotlinx.coroutines.CancellationException
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject
import java.util.concurrent.TimeUnit

/**
 * Parity wave B (16 §3.1, worker r, checklist home.calendar.reminders): every 30 min, with network, asks [AiringRepositoryImpl.dueReminders]
 * which followed episodes (`remind == true`) aired since its previous run and posts one notification each on channel
 * [AiringRepositoryImpl.CHANNEL_ID] (`airing_notification_title`, tap = launcher intent with `NotificationExtras.DETAILS_ANILIST_ID`).
 * Scheduled by [DiscoverScheduler] while `AppSettings.airingReminders`; cancelled when it is off. Koin is started by
 * `TatakaiApplication` before WorkManager's default initializer (same as `HistorySyncWorker`).
 */
class AiringReminderWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params), KoinComponent {
    private val airing: AiringRepositoryImpl by inject()

    override suspend fun doWork(): Result = try {
        val due = airing.dueReminders()
        due.take(MAX_PER_RUN).forEach { notify(applicationContext, it) }
        Result.success()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        Result.retry()
    }

    companion object {
        const val UNIQUE_NAME: String = "tatakai-airing-reminders"
        const val INTERVAL_MINUTES: Long = 30
        private const val MAX_PER_RUN = 8
        private const val NOTIFICATION_BASE = 0x5a00

        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<AiringReminderWorker>(INTERVAL_MINUTES, TimeUnit.MINUTES)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(UNIQUE_NAME, ExistingPeriodicWorkPolicy.KEEP, request)
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(UNIQUE_NAME)
        }

        fun ensureChannel(context: Context) {
            val channel = NotificationChannelCompat.Builder(AiringRepositoryImpl.CHANNEL_ID, NotificationManagerCompat.IMPORTANCE_DEFAULT)
                .setName(context.getString(R.string.airing_channel))
                .setDescription(context.getString(R.string.airing_channel_desc))
                .build()
            NotificationManagerCompat.from(context).createNotificationChannel(channel)
        }

        /** One "<title> · episode N" notification; tap opens Details of the series. */
        fun notify(context: Context, ep: AiringEpisode) {
            val granted = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
            if (!granted) return
            val id = ep.media.ref.anilistId ?: ep.media.ref.malId ?: 0
            val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                ep.media.ref.anilistId?.let { putExtra(NotificationExtras.DETAILS_ANILIST_ID, it) }
            }
            val pending = launch?.let {
                PendingIntent.getActivity(context, NOTIFICATION_BASE + (id and 0xfff), it, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
            }
            val notification = NotificationCompat.Builder(context, AiringRepositoryImpl.CHANNEL_ID)
                .setSmallIcon(android.R.drawable.ic_popup_reminder)
                .setContentTitle(context.getString(R.string.airing_notification_title, ep.media.title, ep.episode))
                .setContentText(context.getString(R.string.airing_notification_body))
                .setAutoCancel(true)
                .setContentIntent(pending)
                .build()
            try {
                ensureChannel(context)
                NotificationManagerCompat.from(context).notify(NOTIFICATION_BASE + ((id * 31 + ep.episode) and 0xfff), notification)
            } catch (_: SecurityException) {
                // permission revoked meanwhile
            }
        }
    }
}
