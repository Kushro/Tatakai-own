// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import android.app.Notification
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.offline.Download
import androidx.media3.exoplayer.offline.DownloadManager
import androidx.media3.exoplayer.offline.DownloadNotificationHelper
import androidx.media3.exoplayer.offline.DownloadService
import androidx.media3.exoplayer.scheduler.PlatformScheduler
import androidx.media3.exoplayer.scheduler.Scheduler
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.DownloadsTab
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject

/**
 * Parity wave B (16 §3.2, worker r, checklist downloads.android.notifications): the Media3 foreground service that drives the
 * `DownloadManager` of [DownloadRepositoryImpl] (Koin `single<DownloadManager>` in `downloadModule`). [PlatformScheduler] (JobScheduler)
 * restarts it when the requirements come back (Wi-Fi only) or after a reboot (`RECEIVE_BOOT_COMPLETED` + the scheduler service in the
 * module manifest). Progress notification: `DownloadNotificationHelper` with the titles of the running episodes; tap opens
 * Downloads › Activas. The "downloaded" / "failed" notifications are posted by the repository ([DownloadNotifications]).
 */
@UnstableApi
class TatakaiDownloadService :
    DownloadService(
        FOREGROUND_NOTIFICATION_ID,
        DEFAULT_FOREGROUND_NOTIFICATION_UPDATE_INTERVAL,
        DownloadRepositoryImpl.CHANNEL_ID,
        R.string.download_service_channel,
        R.string.download_service_channel_desc,
    ),
    KoinComponent {
    private val manager: DownloadManager by inject()
    private val downloads: DownloadRepository by inject()
    private val helper by lazy { DownloadNotificationHelper(this, DownloadRepositoryImpl.CHANNEL_ID) }

    override fun getDownloadManager(): DownloadManager = manager

    override fun getScheduler(): Scheduler? = runCatching { PlatformScheduler(this, JOB_ID) }.getOrNull()

    override fun getForegroundNotification(downloads: MutableList<Download>, notMetRequirements: Int): Notification {
        val titles = this.downloads.items.value.associate { it.id to "${it.title} · ${it.episode}" }
        val running = downloads.filter { it.state == Download.STATE_DOWNLOADING || it.state == Download.STATE_QUEUED }
        val message = when {
            notMetRequirements != 0 -> getString(R.string.download_service_waiting_wifi)
            running.size == 1 -> titles[running.first().request.id]
            running.isNotEmpty() -> getString(R.string.download_service_progress, running.size)
            else -> null
        }
        return helper.buildProgressNotification(
            this,
            android.R.drawable.stat_sys_download,
            DownloadNotifications.openDownloads(this, DownloadsTab.ACTIVE, FOREGROUND_NOTIFICATION_ID),
            message,
            downloads,
            notMetRequirements,
        )
    }

    companion object {
        const val FOREGROUND_NOTIFICATION_ID: Int = 4101
        const val DONE_NOTIFICATION_ID: Int = 4102
        const val JOB_ID: Int = 4100
    }
}
