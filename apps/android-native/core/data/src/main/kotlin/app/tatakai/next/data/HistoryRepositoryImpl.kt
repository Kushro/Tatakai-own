package app.tatakai.next.data

import app.tatakai.next.database.HistoryDao
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.supabase.SupabaseNotConfiguredException
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map

/**
 * Local-first: every [record] hits Room immediately; the server copy is written by [HistorySyncer] through a
 * coalesced WorkManager job (at most one request per 15 s window).
 */
class HistoryRepositoryImpl(
    private val dao: HistoryDao,
    private val scheduler: HistorySyncScheduler,
    /** android wave: null = no sync available (tests); [syncNow] is then a successful no-op. */
    private val syncer: HistorySyncer? = null,
) : HistoryRepository {

    /** Legacy rule: drop completed rows, keep ONE row per anime (the most recently watched episode). */
    override fun continueWatching(limit: Int): Flow<List<HistoryEntry>> =
        dao.observeRecent(RECENT_WINDOW).map { rows ->
            rows.asSequence()
                .filter { !it.completed }
                .distinctBy { it.animeId }
                .take(limit)
                .map { it.toEntry() }
                .toList()
        }

    override suspend fun record(entry: HistoryEntry) {
        val existing = dao.get(entry.episodeId)
        // A "started" seed (progress 0) must never overwrite a real position that is already stored.
        if (existing != null && entry.progressSec == 0 && existing.progressSec > 0) return
        dao.upsert(entry.toRow())
        scheduler.requestSync(immediate = false)
    }

    override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> =
        dao.observeForAnime(anilistId, anilistId.toString()).map { rows ->
            rows.associate { it.episode to it.toEntry() }
        }

    // --- android wave (12 §3.6) ---

    /** Library › Historial: one row per watched episode, newest first. */
    override fun history(limit: Int): Flow<List<HistoryEntry>> =
        dao.observeRecent(limit.coerceAtLeast(1)).map { rows -> rows.map { it.toEntry() } }

    /** Push pending rows and pull the remote ones now. Signed out / Supabase not configured = success (nothing to do). */
    override suspend fun syncNow(): Result<Unit> {
        val s = syncer ?: return Result.success(Unit)
        return try {
            s.syncOnce()
            Result.success(Unit)
        } catch (e: CancellationException) {
            throw e
        } catch (_: SupabaseNotConfiguredException) {
            Result.success(Unit)
        } catch (e: Exception) {
            // SessionExpiredException included: the UI shows the error and the account card offers to sign in again
            Result.failure(e)
        }
    }

    override val lastSyncedAt: StateFlow<Long?> = syncer?.lastSyncedAt ?: MutableStateFlow<Long?>(null).asStateFlow()

    private companion object {
        const val RECENT_WINDOW = 200
    }
}
