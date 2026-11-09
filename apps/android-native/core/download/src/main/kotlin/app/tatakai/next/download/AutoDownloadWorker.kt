// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import app.tatakai.next.model.DownloadRepository
import kotlinx.coroutines.CancellationException
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject
import java.util.concurrent.TimeUnit

/**
 * Parity wave B (16 §3.2, worker r, checklist downloads.auto.schedule / downloads.android.workmanager): every 30 min (WorkManager
 * minimum is 15; classic polls every 30), only on unmetered network with battery not low, runs `DownloadRepository.checkNow()` for the
 * enabled rules. Scheduled by [schedule] from `downloadModule` when `AppSettings.downloads.autoCheck` is on; cancelled when it is off.
 * Koin is started by `TatakaiApplication` before WorkManager's default initializer (same as `HistorySyncWorker`).
 */
class AutoDownloadWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params), KoinComponent {
    private val downloads: DownloadRepository by inject()

    override suspend fun doWork(): Result = try {
        downloads.checkNow()
        Result.success()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        Result.retry()
    }

    companion object {
        const val UNIQUE_NAME: String = "tatakai-auto-download"
        const val INTERVAL_MINUTES: Long = 30

        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<AutoDownloadWorker>(INTERVAL_MINUTES, TimeUnit.MINUTES)
                .setConstraints(
                    Constraints.Builder()
                        .setRequiredNetworkType(NetworkType.UNMETERED)
                        .setRequiresBatteryNotLow(true)
                        .build(),
                )
                .build()
            WorkManager.getInstance(context).enqueueUniquePeriodicWork(UNIQUE_NAME, ExistingPeriodicWorkPolicy.KEEP, request)
        }

        fun cancel(context: Context) {
            WorkManager.getInstance(context).cancelUniqueWork(UNIQUE_NAME)
        }
    }
}
