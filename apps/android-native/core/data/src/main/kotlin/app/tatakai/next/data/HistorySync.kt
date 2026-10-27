package app.tatakai.next.data

import android.content.Context
import androidx.work.BackoffPolicy
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingWorkPolicy
import androidx.work.NetworkType
import androidx.work.OneTimeWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import app.tatakai.next.database.HistoryDao
import app.tatakai.next.supabase.SessionExpiredException
import app.tatakai.next.supabase.SupabaseAuthSource
import app.tatakai.next.supabase.SupabaseNotConfiguredException
import app.tatakai.next.supabase.WatchHistoryRemote
import kotlinx.coroutines.CancellationException
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject
import java.util.concurrent.TimeUnit

/** Asks for a (coalesced) push of dirty history rows. */
interface HistorySyncScheduler {
    /**
     * [immediate] = false: at most one request per [HistorySyncWorker.COALESCE_SECONDS] window (normal progress ticks).
     * [immediate] = true: run as soon as possible (pause, leaving the player, sign-in). Player code can call
     * `get<HistorySyncScheduler>().requestSync(true)` on pause / exit.
     */
    fun requestSync(immediate: Boolean = false)

    /** Called by the worker itself when rows were written during its run (a plain KEEP would be dropped). */
    fun requestFollowUp() = requestSync(immediate = false)
}

class WorkManagerHistorySyncScheduler(private val context: Context) : HistorySyncScheduler {
    override fun requestFollowUp() = enqueue(immediate = false, name = HistorySyncWorker.UNIQUE_NAME + "-next")

    override fun requestSync(immediate: Boolean) = enqueue(immediate, HistorySyncWorker.UNIQUE_NAME)

    private fun enqueue(immediate: Boolean, name: String) {
        val request = OneTimeWorkRequestBuilder<HistorySyncWorker>()
            .setConstraints(Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build())
            .setBackoffCriteria(BackoffPolicy.EXPONENTIAL, 30, TimeUnit.SECONDS)
            .apply { if (!immediate) setInitialDelay(HistorySyncWorker.COALESCE_SECONDS, TimeUnit.SECONDS) }
            .build()
        // KEEP: while a delayed job is pending, further writes join it (coalescing). REPLACE: flush now.
        val policy = if (immediate) ExistingWorkPolicy.REPLACE else ExistingWorkPolicy.KEEP
        WorkManager.getInstance(context).enqueueUniqueWork(name, policy, request)
    }
}

/**
 * One sync pass: push dirty rows (batched upsert, `on_conflict=user_id,episode_id`), then pull the 30 most recent
 * server rows and merge them (newest `watched_at` wins, tie -> higher progress).
 */
class HistorySyncer(
    private val dao: HistoryDao,
    private val remote: WatchHistoryRemote,
    private val auth: SupabaseAuthSource,
) {
    enum class Outcome { SKIPPED_SIGNED_OUT, SYNCED }

    /** @throws SessionExpiredException when the token cannot be refreshed; other exceptions mean "retry later". */
    suspend fun syncOnce(): Outcome {
        if (auth.session.value == null) return Outcome.SKIPPED_SIGNED_OUT
        while (true) {
            val batch = dao.dirty(BATCH)
            if (batch.isEmpty()) break
            remote.upsert(batch.map { it.toRemote() })
            // Only rows not touched again while the request was in flight become clean.
            batch.forEach { dao.markSynced(it.episodeId, it.watchedAtMs) }
            if (batch.size < BATCH) break
        }
        remote.recent(PULL_LIMIT).forEach { dao.mergeRemote(it.toRow()) }
        return Outcome.SYNCED
    }

    suspend fun hasPending(): Boolean = dao.dirtyCount() > 0

    private companion object {
        const val BATCH = 50
        const val PULL_LIMIT = 30
    }
}

class HistorySyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params), KoinComponent {
    private val syncer: HistorySyncer by inject()
    private val scheduler: HistorySyncScheduler by inject()

    override suspend fun doWork(): Result = try {
        syncer.syncOnce()
        // Rows written while this pass ran: schedule the next coalesced window.
        if (syncer.hasPending()) scheduler.requestFollowUp()
        Result.success()
    } catch (e: CancellationException) {
        throw e
    } catch (_: SessionExpiredException) {
        Result.success() // signed out: nothing to push until the user signs in again
    } catch (_: SupabaseNotConfiguredException) {
        Result.success()
    } catch (_: Exception) {
        Result.retry()
    }

    companion object {
        const val UNIQUE_NAME = "tatakai-history-sync"
        const val COALESCE_SECONDS = 15L
    }
}
