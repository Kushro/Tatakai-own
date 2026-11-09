package app.tatakai.next.data

import app.tatakai.next.database.HistoryDao
import app.tatakai.next.link.LinkHistoryRow
import app.tatakai.next.link.LinkHistoryUnavailableException
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map

/**
 * Local-first: every [record] hits Room immediately (the exact resume position, `history.local-resume`); the paired PC's copy is
 * written by [HistorySyncer] through a coalesced WorkManager job (at most one request per 15 s window).
 * parity wave A (worker q): a row that becomes completed advances the AniList / MAL progress ([lists], `AppSettings.autoListProgress`)
 * and the first play of a title off the list adds it as WATCHING (`autoListAdd`).
 */
class HistoryRepositoryImpl(
    private val dao: HistoryDao,
    private val scheduler: HistorySyncScheduler,
    /** android wave: null = no sync available (tests); [syncNow] is then a successful no-op. */
    private val syncer: HistorySyncer? = null,
    private val lists: ListsRepository? = null,
    private val autoListProgress: () -> Boolean = { false },
    private val autoListAdd: () -> Boolean = { false },
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
        val row = entry.toRow()
        dao.upsert(row)
        scheduler.requestSync(immediate = false)
        listHooks(entry, newlyCompleted = row.completed && existing?.completed != true, firstPlay = existing == null)
    }

    /** Best effort: the lists layer never breaks the history. */
    private suspend fun listHooks(entry: HistoryEntry, newlyCompleted: Boolean, firstPlay: Boolean) {
        val repo = lists ?: return
        val ref = MediaRef(anilistId = entry.anilistId, malId = entry.malId)
        if (ref.anilistId == null && ref.malId == null) return
        runCatching {
            if (firstPlay && autoListAdd() && repo.entry(ref).first() == null) {
                repo.setStatus(entry.toMedia(ref), ListStatus.WATCHING)
            }
            if (newlyCompleted && autoListProgress()) repo.onEpisodeCompleted(ref, entry.episode)
        }
    }

    private fun HistoryEntry.toMedia(ref: MediaRef) = Media(
        ref = ref, kind = MediaKind.ANIME, title = title, titles = emptyMap(), cover = poster, banner = null, color = null,
        synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null,
        sourceProvider = "history",
    )

    override fun progressFor(anilistId: Int): Flow<Map<Int, HistoryEntry>> =
        dao.observeForAnime(anilistId, anilistId.toString()).map { rows ->
            rows.associate { it.episode to it.toEntry() }
        }

    // --- android wave (12 §3.6) ---

    /** Library › Historial: one row per watched episode, newest first. */
    override fun history(limit: Int): Flow<List<HistoryEntry>> =
        dao.observeRecent(limit.coerceAtLeast(1)).map { rows -> rows.map { it.toEntry() } }

    /** Push dirty rows to the paired PC and pull its recent ones now (Tatakai Link). No PC paired / sync disabled = success (nothing to do). */
    override suspend fun syncNow(): Result<Unit> {
        val s = syncer ?: return Result.success(Unit)
        return try {
            s.syncOnce()
            Result.success(Unit)
        } catch (e: CancellationException) {
            throw e
        } catch (_: LinkHistoryUnavailableException) {
            Result.success(Unit)
        } catch (e: Exception) {
            // LinkException (PC offline, busy): the UI shows the error and offers to retry
            Result.failure(e)
        }
    }

    override val lastSyncedAt: StateFlow<Long?> = syncer?.lastSyncedAt ?: MutableStateFlow<Long?>(null).asStateFlow()

    // --- parity wave A (15 §3.4, worker q) ---

    /** JSON array of every local row in the Tatakai Link `/v1/history` shape (camelCase `HistoryEntry`): desktop imports it as is. */
    override suspend fun exportJson(): String = withContext(Dispatchers.IO) {
        EXPORT_JSON.encodeToString(ListSerializer(LinkHistoryRow.serializer()), dao.all().map { it.toRemote() })
    }

    /** Local rows always; with [includeRemote] also the paired PC's copy (`DELETE /v1/history`), which the UI confirms first. */
    override suspend fun clearHistory(includeRemote: Boolean): Result<Unit> = try {
        if (includeRemote) {
            try {
                syncer?.clearRemote()
            } catch (_: LinkHistoryUnavailableException) {
                // no PC: only the local copy exists
            }
        }
        dao.clear()
        Result.success(Unit)
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e)
    }

    private companion object {
        const val RECENT_WINDOW = 200
        val EXPORT_JSON = Json {
            prettyPrint = true
            encodeDefaults = true
            explicitNulls = true
        }
    }
}
