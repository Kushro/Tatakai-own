// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

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
import app.tatakai.next.model.LinkHandoff
import app.tatakai.next.model.NotificationExtras

/**
 * Parity wave B (16 §3.11, worker r, checklist link.send-to-device): "La PC te envió <título>" on channel `link`. Tap = launcher intent
 * with `NotificationExtras.LINK_HANDOFF = true` (the shell opens `Route.Linked`, which shows "Reproducir aquí").
 */
fun interface LinkHandoffNotifier {
    fun notify(handoff: LinkHandoff)
}

class AndroidLinkHandoffNotifier(private val context: Context) : LinkHandoffNotifier {
    override fun notify(handoff: LinkHandoff) {
        val granted = Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED
        if (!granted) return
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)?.apply {
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            putExtra(NotificationExtras.LINK_HANDOFF, true)
        }
        val pending = launch?.let { PendingIntent.getActivity(context, NOTIFICATION_ID, it, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE) }
        val title = handoff.title?.takeIf { it.isNotBlank() }
        val notification = NotificationCompat.Builder(context, CHANNEL_ID)
            .setSmallIcon(android.R.drawable.ic_media_play)
            .setContentTitle(if (title != null) context.getString(R.string.link_handoff_title, title) else context.getString(R.string.link_handoff_title_generic))
            .setContentText(
                handoff.episode?.let { context.getString(R.string.link_handoff_body_episode, it) } ?: context.getString(R.string.link_handoff_body),
            )
            .setAutoCancel(true)
            .setContentIntent(pending)
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .build()
        try {
            val channel = NotificationChannelCompat.Builder(CHANNEL_ID, NotificationManagerCompat.IMPORTANCE_HIGH)
                .setName(context.getString(R.string.link_handoff_channel))
                .setDescription(context.getString(R.string.link_handoff_channel_desc))
                .build()
            val manager = NotificationManagerCompat.from(context)
            manager.createNotificationChannel(channel)
            manager.notify(NOTIFICATION_ID, notification)
        } catch (_: SecurityException) {
            // permission revoked meanwhile
        }
    }

    companion object {
        const val CHANNEL_ID = "link"
        const val NOTIFICATION_ID = 0x4c01
    }
}
