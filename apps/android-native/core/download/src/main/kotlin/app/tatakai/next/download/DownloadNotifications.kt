// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

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
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadsTab
import app.tatakai.next.model.NotificationExtras

/**
 * Parity wave B (16 §3.2, worker r, checklist downloads.android.notifications): channel `downloads` (also used by the foreground
 * progress of [TatakaiDownloadService]) and the one-off "episode downloaded" / "download failed" notifications. Tap = launcher intent
 * with `NotificationExtras.DOWNLOADS_TAB` (the shell opens `Route.Downloads(tab)` through `WaveBLinks`).
 */
internal object DownloadNotifications {
    private const val BASE_ID = 0x4200

    fun ensureChannel(context: Context) {
        val channel = NotificationChannelCompat.Builder(DownloadRepositoryImpl.CHANNEL_ID, NotificationManagerCompat.IMPORTANCE_LOW)
            .setName(context.getString(R.string.download_service_channel))
            .setDescription(context.getString(R.string.download_service_channel_desc))
            .build()
        NotificationManagerCompat.from(context).createNotificationChannel(channel)
    }

    /** Launcher intent that opens Downloads on [tab]. */
    fun openDownloads(context: Context, tab: DownloadsTab, requestCode: Int): PendingIntent? {
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            putExtra(NotificationExtras.DOWNLOADS_TAB, tab.id)
        } ?: return null
        return PendingIntent.getActivity(context, requestCode, launch, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
    }

    fun done(context: Context, item: DownloadItem) = post(
        context,
        item,
        context.getString(R.string.download_service_done, item.title, item.episode),
        android.R.drawable.stat_sys_download_done,
        DownloadsTab.ANIME,
    )

    fun failed(context: Context, item: DownloadItem) = post(
        context,
        item,
        context.getString(R.string.download_service_failed, item.title, item.episode),
        android.R.drawable.stat_notify_error,
        DownloadsTab.HISTORY,
    )

    private fun post(context: Context, item: DownloadItem, title: String, icon: Int, tab: DownloadsTab) {
        val granted = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        if (!granted) return
        val id = BASE_ID + (item.id.hashCode() and 0xfff)
        val notification = NotificationCompat.Builder(context, DownloadRepositoryImpl.CHANNEL_ID)
            .setSmallIcon(icon)
            .setContentTitle(title)
            .setContentText(item.sourceLabel)
            .setAutoCancel(true)
            .setContentIntent(openDownloads(context, tab, id))
            .build()
        try {
            ensureChannel(context)
            NotificationManagerCompat.from(context).notify(id, notification)
        } catch (_: SecurityException) {
            // permission revoked meanwhile
        }
    }
}
