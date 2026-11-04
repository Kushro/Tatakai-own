// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.system

import android.Manifest
import android.app.Notification
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationChannelCompat
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import app.tatakai.next.torrent.R
import app.tatakai.next.torrent.TorrentService

/** Aggregated engine state the foreground notification and the service need (internal; derived by TorrentRepositoryImpl). */
data class ServiceSnapshot(
    /** Something streams, plays or downloads: the service must stay in the foreground. */
    val demand: Boolean = false,
    val streaming: Boolean = false,
    /** Title of the streamed episode (label title), when streaming. */
    val streamingTitle: String? = null,
    /** Torrents transferring in the background (not streaming). */
    val downloads: Int = 0,
    val downBps: Long = 0,
    val upBps: Long = 0,
    /** Aggregated 0..1 progress of the background downloads; null = none. */
    val progress: Double? = null,
    val pausedBySystem: Boolean = false,
)

/** Notification channel `torrents` (importance low) and the notifications of 11 §5.2 / §4.3. */
object TorrentNotifications {
    const val FOREGROUND_ID: Int = 0x7a7a
    private const val EVENTS_ID_BASE: Int = 0x7b00

    /** Extra of the launch intent: section of the torrent client to open (`TorrentSection.id`), for the app shell (worker b). */
    const val EXTRA_SECTION: String = "app.tatakai.next.torrent.SECTION"

    /** Extra of the launch intent: torrent id (info-hash) to open. */
    const val EXTRA_TORRENT_ID: String = "app.tatakai.next.torrent.TORRENT_ID"

    fun ensureChannel(context: Context) {
        val channel = NotificationChannelCompat.Builder(TorrentService.CHANNEL_ID, NotificationManagerCompat.IMPORTANCE_LOW)
            .setName(context.getString(R.string.torrent_notif_channel_name))
            .setDescription(context.getString(R.string.torrent_notif_channel_desc))
            .setShowBadge(false)
            .build()
        NotificationManagerCompat.from(context).createNotificationChannel(channel)
    }

    fun foreground(context: Context, snapshot: ServiceSnapshot, formatRate: (Long) -> String): Notification {
        val title = when {
            snapshot.streaming -> context.getString(
                R.string.torrent_notif_title_streaming,
                snapshot.streamingTitle ?: context.getString(R.string.torrent_notif_channel_name),
            )
            snapshot.downloads > 0 -> context.resources.getQuantityString(
                R.plurals.torrent_notif_title_downloads,
                snapshot.downloads,
                snapshot.downloads,
            )
            else -> context.getString(R.string.torrent_notif_title_idle)
        }
        val builder = NotificationCompat.Builder(context, TorrentService.CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle(title)
            .setContentText(context.getString(R.string.torrent_notif_speed, formatRate(snapshot.downBps), formatRate(snapshot.upBps)))
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setCategory(NotificationCompat.CATEGORY_PROGRESS)
            .setForegroundServiceBehavior(NotificationCompat.FOREGROUND_SERVICE_IMMEDIATE)
            .setContentIntent(openApp(context, section = if (snapshot.streaming) null else "downloads", torrentId = null, requestCode = 1))
            .addAction(0, context.getString(R.string.torrent_notif_open), openApp(context, "downloads", null, 2))
        val progress = snapshot.progress
        if (!snapshot.streaming && progress != null) {
            builder.setProgress(PROGRESS_MAX, (progress.coerceIn(0.0, 1.0) * PROGRESS_MAX).toInt(), false)
        }
        if (snapshot.downloads > 0) {
            builder.addAction(0, context.getString(R.string.torrent_notif_pause_all), serviceAction(context, TorrentService.ACTION_PAUSE_ALL, 3))
        }
        return builder.build()
    }

    /** "Descargas pausadas por el sistema; toca para reanudar" (Android 15 dataSync `onTimeout`). */
    fun notifyPausedBySystem(context: Context) {
        val notification = NotificationCompat.Builder(context, TorrentService.CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_warning)
            .setContentTitle(context.getString(R.string.torrent_android_service_paused_title))
            .setContentText(context.getString(R.string.torrent_android_service_paused_body))
            .setStyle(NotificationCompat.BigTextStyle().bigText(context.getString(R.string.torrent_android_service_paused_body)))
            .setAutoCancel(true)
            .setContentIntent(openApp(context, "downloads", null, 4))
            .addAction(0, context.getString(R.string.torrent_notif_resume), serviceAction(context, TorrentService.ACTION_RESUME_ALL, 5))
            .build()
        post(context, EVENTS_ID_BASE, notification)
    }

    /** "Episodio N listo" with "Ver" (opens the app on the torrent; the shell plays the local file). */
    fun notifyCompleted(context: Context, torrentId: String, title: String, episode: Int?) {
        val text = if (episode != null) context.getString(R.string.torrent_notif_episode_ready, episode) else context.getString(R.string.torrent_notif_download_ready)
        val open = openApp(context, "overview", torrentId, 6 + (torrentId.hashCode() and 0xffff))
        val notification = NotificationCompat.Builder(context, TorrentService.CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_download_done)
            .setContentTitle(title)
            .setContentText(text)
            .setAutoCancel(true)
            .setContentIntent(open)
            .addAction(0, context.getString(R.string.torrent_notif_watch), open)
            .build()
        post(context, EVENTS_ID_BASE + 1 + (torrentId.hashCode() and 0xfff), notification)
    }

    /** One-off error (no space, no matching file in a background download). */
    fun notifyError(context: Context, torrentId: String, message: String) {
        val notification = NotificationCompat.Builder(context, TorrentService.CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_warning)
            .setContentTitle(context.getString(R.string.torrent_notif_error_title))
            .setContentText(message)
            .setAutoCancel(true)
            .setContentIntent(openApp(context, "overview", torrentId, 7 + (torrentId.hashCode() and 0xffff)))
            .build()
        post(context, EVENTS_ID_BASE + 0x1000 + (torrentId.hashCode() and 0xfff), notification)
    }

    private fun post(context: Context, id: Int, notification: Notification) {
        val granted = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        if (!granted) return
        try {
            ensureChannel(context)
            NotificationManagerCompat.from(context).notify(id, notification)
        } catch (_: SecurityException) {
            // permission revoked meanwhile
        }
    }

    private fun openApp(context: Context, section: String?, torrentId: String?, requestCode: Int): PendingIntent? {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName) ?: return null
        launch.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        if (section != null) launch.putExtra(EXTRA_SECTION, section)
        if (torrentId != null) launch.putExtra(EXTRA_TORRENT_ID, torrentId)
        return PendingIntent.getActivity(context, requestCode, launch, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }

    private fun serviceAction(context: Context, action: String, requestCode: Int): PendingIntent {
        val intent = Intent(context, TorrentService::class.java).setAction(action)
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            PendingIntent.getForegroundService(context, requestCode, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        } else {
            PendingIntent.getService(context, requestCode, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        }
    }

    private const val PROGRESS_MAX = 1000
}
