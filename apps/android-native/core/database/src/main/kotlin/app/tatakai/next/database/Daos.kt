package app.tatakai.next.database

import androidx.room.Dao
import androidx.room.Insert
import androidx.room.OnConflictStrategy
import androidx.room.Query
import androidx.room.Transaction
import kotlinx.coroutines.flow.Flow

@Dao
abstract class HistoryDao {
    @Query("SELECT * FROM history ORDER BY watchedAtMs DESC LIMIT :limit")
    abstract fun observeRecent(limit: Int): Flow<List<HistoryRow>>

    @Query("SELECT * FROM history WHERE anilistId = :anilistId OR animeId = :animeId")
    abstract fun observeForAnime(anilistId: Int, animeId: String): Flow<List<HistoryRow>>

    @Query("SELECT * FROM history WHERE episodeId = :episodeId")
    abstract suspend fun get(episodeId: String): HistoryRow?

    @Query("SELECT * FROM history WHERE syncedAtMs IS NULL OR syncedAtMs != watchedAtMs ORDER BY watchedAtMs ASC LIMIT :limit")
    abstract suspend fun dirty(limit: Int): List<HistoryRow>

    @Query("SELECT COUNT(*) FROM history WHERE syncedAtMs IS NULL OR syncedAtMs != watchedAtMs")
    abstract suspend fun dirtyCount(): Int

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    abstract suspend fun upsert(row: HistoryRow)

    /** Marks as synced only when the row was not updated again while the request was in flight. */
    @Query("UPDATE history SET syncedAtMs = :watchedAtMs WHERE episodeId = :episodeId AND watchedAtMs = :watchedAtMs")
    abstract suspend fun markSynced(episodeId: String, watchedAtMs: Long)

    /** Merge a server row: newest watchedAt wins; on a tie the higher progress wins. Returns true if stored. */
    @Transaction
    open suspend fun mergeRemote(remote: HistoryRow): Boolean {
        val local = get(remote.episodeId)
        val takeRemote = local == null ||
            remote.watchedAtMs > local.watchedAtMs ||
            (remote.watchedAtMs == local.watchedAtMs && remote.progressSec > local.progressSec)
        if (takeRemote) {
            upsert(remote.copy(syncedAtMs = remote.watchedAtMs))
        } else if (local != null && local.watchedAtMs == remote.watchedAtMs && local.progressSec == remote.progressSec) {
            upsert(local.copy(syncedAtMs = local.watchedAtMs))
        }
        return takeRemote
    }

    @Query("DELETE FROM history")
    abstract suspend fun clear()
}

@Dao
interface FeedCacheDao {
    @Query("SELECT * FROM feed_cache WHERE key = :key")
    suspend fun get(key: String): FeedCacheRow?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun put(row: FeedCacheRow)

    @Query("DELETE FROM feed_cache WHERE fetchedAtMs < :olderThanMs")
    suspend fun purgeOlderThan(olderThanMs: Long)

    @Query("DELETE FROM feed_cache")
    suspend fun clear()
}
