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
import app.tatakai.next.link.LinkException
import app.tatakai.next.link.LinkHistoryRemote
import app.tatakai.next.link.LinkHistoryUnavailableException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.StateFlow
import org.koin.core.component.KoinComponent
import org.koin.core.component.inject
import java.util.concurrent.TimeUnit

// parity wave A, 2026-11-09 amendment (plan 14 §0.1 #9): the server side of the history sync is the paired PC over Tatakai Link
// (`LinkHistoryRemote`, :core:link), not Supabase. Same local-first model: Room is the source of truth, dirty rows are pushed in
// coalesced WorkManager passes, recent PC rows are merged back. Owner: worker q.

/** Asks for a (coalesced) push of dirty history rows. */
interface HistorySyncScheduler {
    /**
     * [immediate] = false: at most one request per [HistorySyncWorker.COALESCE_SECONDS] window (normal progress ticks).
     * [immediate] = true: run as soon as possible (pause, leaving the player, pairing a PC). Player code can call
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
 * One sync pass against the paired PC (desktop contract §3.8): push dirty rows (batched `POST /v1/history`), then pull the rows the PC
 * changed after the stored cursor (`GET /v1/history?since=<unix s>`, oldest first, repeating while `truncated`) and merge them
 * (newest `watchedAt` wins, tie -> higher progress: the same `HistoryEntry::wins_over` rule the PC applies). The PC is only a peer with
 * the same merge rule, so two phones paired to one PC converge through it. Runs when the app opens, when an episode ends (the
 * player's `HistoryFlush`) and on the coalesced progress ticks.
 */
class HistorySyncer(
    private val dao: HistoryDao,
    private val remote: LinkHistoryRemote,
    /** `AppSettings.historySyncOverLink`; false = behave as unpaired (nothing is sent). */
    private val enabled: () -> Boolean = { true },
    // android wave (12 §3.6): stamp of the last successful pass, exposed as HistoryRepository.lastSyncedAt
    private val stamp: HistorySyncStamp = HistorySyncStamp(),
    /** Pull cursor (`next_since_unix` of the last page, unix seconds); worker q persists it beside the stamp (DataStore key `history_link_since_unix`). */
    private val cursor: () -> Long = { 0L },
    private val saveCursor: suspend (Long) -> Unit = {},
    private val clock: () -> Long = System::currentTimeMillis,
) {
    enum class Outcome { SKIPPED_NOT_PAIRED, SKIPPED_DISABLED, SYNCED }

    /** Epoch ms of the last successful [syncOnce]; null = never / no paired PC. */
    val lastSyncedAt: StateFlow<Long?> get() = stamp.value

    /** Forget: the stamp and cursor belong to the pairing that left. */
    suspend fun resetStamp() {
        stamp.set(null)
        saveCursor(0L)
    }

    /**
     * @throws LinkHistoryUnavailableException when the pairing is gone / revoked (treat as not paired);
     *   [LinkException] or any other exception means "retry later".
     */
    suspend fun syncOnce(): Outcome {
        if (!enabled()) return Outcome.SKIPPED_DISABLED
        if (!remote.available) return Outcome.SKIPPED_NOT_PAIRED
        while (true) {
            val batch = dao.dirty(BATCH)
            if (batch.isEmpty()) break
            remote.push(batch.map { it.toRemote() })
            // Only rows not touched again while the request was in flight become clean.
            batch.forEach { dao.markSynced(it.episodeId, it.watchedAtMs) }
            if (batch.size < BATCH) break
        }
        var since = cursor()
        while (true) {
            val page = remote.since(since)
            page.items.forEach { dao.mergeRemote(it.toRow()) }
            since = page.nextSinceUnix
            if (!page.truncated) break
        }
        saveCursor(since)
        stamp.set(clock())
        return Outcome.SYNCED
    }

    suspend fun hasPending(): Boolean = dao.dirtyCount() > 0

    private companion object {
        /** <= `LinkClient.MAX_HISTORY_ROWS` (500) and comfortably under the 256 KiB body cap. */
        const val BATCH = 100
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
    } catch (_: LinkHistoryUnavailableException) {
        Result.success() // no PC: nothing to push until the user pairs one again
    } catch (_: LinkException) {
        Result.retry() // PC offline / busy: exponential backoff
    } catch (_: Exception) {
        Result.retry()
    }

    companion object {
        const val UNIQUE_NAME = "tatakai-history-sync"
        const val COALESCE_SECONDS = 15L
    }
}
