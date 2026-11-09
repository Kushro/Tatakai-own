// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

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
import app.tatakai.next.model.NotificationExtras
import app.tatakai.next.model.SocialRepository
import kotlinx.coroutines.CancellationException
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject
import java.io.File
import java.util.concurrent.TimeUnit

/**
 * Parity wave B (16 §3.7, worker t, checklist social.notifications.badge): every 15 min (WorkManager minimum) with network, refreshes
 * `SocialRepository.unreadCount`; when it grew since the previous run, posts ONE summary notification (`social_notification_unread`,
 * channel `SocialRepositoryImpl.CHANNEL_ID`, tap = launcher intent with `NotificationExtras.SOCIAL_NOTIFICATIONS = true`). The badge in
 * the shell reads `unreadCount` directly; the foreground refresh is done by the screens (`refreshUnread()` on resume). Scheduled while
 * `AppSettings.socialNotifications` and an AniList account is linked ([SocialScheduler]).
 */
class SocialNotificationsWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params), KoinComponent {
    private val social: SocialRepository by inject()

    override suspend fun doWork(): Result = try {
        social.refreshUnread()
        val count = social.unreadCount.value
        val store = LastNotifiedStore(File(applicationContext.cacheDir, SocialRepositoryImpl.CACHE_DIR))
        if (shouldNotify(count, store.read())) post(applicationContext, count)
        store.write(count)
        Result.success()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        Result.retry()
    }

    companion object {
        const val UNIQUE_NAME: String = "tatakai-social-notifications"
        const val INTERVAL_MINUTES: Long = 15
        private const val NOTIFICATION_ID = 0x50c1

        /** A summary is posted only when the unread count grew since the last one we showed (0 resets it). */
        fun shouldNotify(count: Int, lastNotified: Int): Boolean = count > 0 && count > lastNotified

        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<SocialNotificationsWorker>(INTERVAL_MINUTES, TimeUnit.MINUTES)
                .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(UNIQUE_NAME, ExistingPeriodicWorkPolicy.KEEP, request)
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(UNIQUE_NAME)
        }

        fun ensureChannel(context: Context) {
            val channel = NotificationChannelCompat.Builder(SocialRepositoryImpl.CHANNEL_ID, NotificationManagerCompat.IMPORTANCE_DEFAULT)
                .setName(context.getString(R.string.social_channel))
                .setDescription(context.getString(R.string.social_channel_desc))
                .build()
            NotificationManagerCompat.from(context).createNotificationChannel(channel)
        }

        private fun post(context: Context, count: Int) {
            val granted = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
            if (!granted) return
            val text = context.resources.getQuantityString(R.plurals.social_notification_unread, count, count)
            val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
                addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                putExtra(NotificationExtras.SOCIAL_NOTIFICATIONS, true)
            }
            val pending = launch?.let { PendingIntent.getActivity(context, NOTIFICATION_ID, it, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE) }
            val notification = NotificationCompat.Builder(context, SocialRepositoryImpl.CHANNEL_ID)
                .setSmallIcon(android.R.drawable.stat_notify_chat)
                .setContentTitle(context.getString(R.string.social_notification_title))
                .setContentText(text)
                .setNumber(count)
                .setAutoCancel(true)
                .setOnlyAlertOnce(true)
                .setCategory(NotificationCompat.CATEGORY_SOCIAL)
                .apply { if (pending != null) setContentIntent(pending) }
                .build()
            try {
                ensureChannel(context)
                NotificationManagerCompat.from(context).notify(NOTIFICATION_ID, notification)
            } catch (_: SecurityException) {
                // permission revoked meanwhile
            }
        }
    }
}

/** The unread count of the last summary posted, in `cacheDir/social` (wiped with the rest of the social cache). */
class LastNotifiedStore(private val dir: File) {
    private val file get() = File(dir, "last_notified.txt")

    fun read(): Int = runCatching { file.readText().trim().toInt() }.getOrDefault(0)

    fun write(count: Int) {
        runCatching {
            dir.mkdirs()
            file.writeText(count.toString())
        }
    }
}
