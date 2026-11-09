// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import android.app.Notification
import androidx.core.app.NotificationCompat
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.offline.Download
import androidx.media3.exoplayer.offline.DownloadManager
import androidx.media3.exoplayer.offline.DownloadService
import androidx.media3.exoplayer.scheduler.Scheduler
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject

/**
 * Parity wave B (16 §3.2, worker r, checklist downloads.android.notifications): the Media3 foreground service that drives the
 * `DownloadManager` of [DownloadRepositoryImpl]. r provides the manager through Koin (`single<DownloadManager>` in `downloadModule`),
 * a `PlatformScheduler` (JobScheduler, in media3-exoplayer; `media3-exoplayer-workmanager` is NOT in the Gradle cache, so WorkManagerScheduler
 * needs a NOTES-deps row) or null (downloads only run while the app process lives), and the progress notification
 * (`DownloadNotificationHelper(context, CHANNEL_ID)`), plus a one-off "episode downloaded" notification whose tap opens
 * `Route.Downloads` (extra [DownloadRepositoryImpl.EXTRA_TAB]). Channel created in `onCreate` (`download_service_channel`).
 *
 * Stub: compiles against Media3 1.11.1 (`DownloadService(int, long, String, int, int)`); `getDownloadManager` resolves the Koin binding
 * that r adds, so until then the service must not be started (the stub repository never starts it).
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

    override fun getDownloadManager(): DownloadManager = manager

    // wave B (r): PlatformScheduler(this, JOB_ID) so paused-by-requirements downloads resume when Wi-Fi is back (needs RECEIVE_BOOT_COMPLETED in the module manifest).
    override fun getScheduler(): Scheduler? = null

    override fun getForegroundNotification(downloads: MutableList<Download>, notMetRequirements: Int): Notification =
        // wave B (r): DownloadNotificationHelper.buildProgressNotification(...) with the titles of the active items.
        NotificationCompat.Builder(this, DownloadRepositoryImpl.CHANNEL_ID)
            .setSmallIcon(android.R.drawable.stat_sys_download)
            .setContentTitle(getString(R.string.download_service_progress, downloads.size))
            .setOngoing(true)
            .build()

    companion object {
        const val FOREGROUND_NOTIFICATION_ID: Int = 4101
        const val DONE_NOTIFICATION_ID: Int = 4102
        const val JOB_ID: Int = 4100
    }
}
