package app.tatakai.next.data

import app.tatakai.next.database.HistoryDao
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.map

/**
 * Local-first: every [record] hits Room immediately; the server copy is written by [HistorySyncer] through a
 * coalesced WorkManager job (at most one request per 15 s window).
 */
class HistoryRepositoryImpl(
    private val dao: HistoryDao,
    private val scheduler: HistorySyncScheduler,
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

    private companion object {
        const val RECENT_WINDOW = 200
    }
}
