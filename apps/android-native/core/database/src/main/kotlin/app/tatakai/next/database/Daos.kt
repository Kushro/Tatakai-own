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

    /** parity wave A (worker q): Settings › Datos › Exportar historial, newest first. */
    @Query("SELECT * FROM history ORDER BY watchedAtMs DESC")
    abstract suspend fun all(): List<HistoryRow>
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

    // --- android wave (12 §3.6): Settings › Metadatos › caché ---
    @Query("SELECT COUNT(*) FROM feed_cache")
    suspend fun count(): Int

    /** UTF-8 bytes of every cached JSON blob. */
    @Query("SELECT COALESCE(SUM(LENGTH(CAST(json AS BLOB))), 0) FROM feed_cache")
    suspend fun totalBytes(): Long
}

/** android wave (12 §3.1): per-source facts behind `SourceStatsStore` (:core:data). */
@Dao
abstract class SourceStatsDao {
    @Query("SELECT * FROM source_stats WHERE extId = :extId AND label = :label")
    abstract suspend fun stat(extId: String, label: String): SourceStatRow?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    abstract suspend fun putStat(row: SourceStatRow)

    @Query("SELECT `key` FROM picked_source WHERE mediaId = :mediaId AND episode = :episode")
    abstract suspend fun picked(mediaId: String, episode: Int): String?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    abstract suspend fun putPicked(row: PickedSourceRow)

    @Query("DELETE FROM picked_source WHERE mediaId = :mediaId AND episode = :episode")
    abstract suspend fun clearPicked(mediaId: String, episode: Int)

    @Query("DELETE FROM source_stats")
    abstract suspend fun clearStats()
}

/** parity wave A (15 §3.3, worker q): lists, offline queue, conflicts, linked accounts, favorite characters. */
@Dao
abstract class ListsDao {
    // --- entries ---
    @Query("SELECT * FROM list_entry WHERE status IS NOT NULL ORDER BY updatedAtMs DESC")
    abstract fun observeListed(): Flow<List<ListEntryRow>>

    @Query("SELECT * FROM list_entry WHERE status = :status ORDER BY updatedAtMs DESC")
    abstract fun observeByStatus(status: String): Flow<List<ListEntryRow>>

    @Query("SELECT * FROM list_entry WHERE favorite = 1 ORDER BY updatedAtMs DESC")
    abstract fun observeFavorites(): Flow<List<ListEntryRow>>

    @Query("SELECT * FROM list_entry WHERE `key` = :key")
    abstract fun observe(key: String): Flow<ListEntryRow?>

    @Query("SELECT * FROM list_entry WHERE `key` = :key")
    abstract suspend fun get(key: String): ListEntryRow?

    @Query("SELECT * FROM list_entry WHERE anilistId = :anilistId LIMIT 1")
    abstract suspend fun byAniList(anilistId: Int): ListEntryRow?

    @Query("SELECT * FROM list_entry WHERE malId = :malId LIMIT 1")
    abstract suspend fun byMal(malId: Int): ListEntryRow?

    @Query("SELECT * FROM list_entry")
    abstract suspend fun all(): List<ListEntryRow>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    abstract suspend fun upsert(row: ListEntryRow)

    @Query("DELETE FROM list_entry WHERE `key` = :key")
    abstract suspend fun delete(key: String)

    // --- offline queue ---
    @Query("SELECT * FROM list_change ORDER BY createdAtMs ASC")
    abstract suspend fun changes(): List<ListChangeRow>

    @Query("SELECT COUNT(*) FROM list_change")
    abstract fun observeChangeCount(): Flow<Int>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    abstract suspend fun putChange(row: ListChangeRow)

    @Query("DELETE FROM list_change WHERE provider = :provider AND `key` = :key AND op = :op")
    abstract suspend fun deleteChange(provider: String, key: String, op: String)

    @Query("DELETE FROM list_change WHERE provider = :provider")
    abstract suspend fun deleteChangesOf(provider: String)

    // --- conflicts ---
    @Query("SELECT * FROM list_conflict ORDER BY createdAtMs ASC")
    abstract fun observeConflicts(): Flow<List<ListConflictRow>>

    @Query("SELECT * FROM list_conflict WHERE id = :id")
    abstract suspend fun conflict(id: String): ListConflictRow?

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    abstract suspend fun putConflict(row: ListConflictRow)

    @Query("DELETE FROM list_conflict WHERE id = :id")
    abstract suspend fun deleteConflict(id: String)

    @Query("DELETE FROM list_conflict WHERE provider = :provider")
    abstract suspend fun deleteConflictsOf(provider: String)

    // --- accounts ---
    @Query("SELECT * FROM linked_account ORDER BY linkedAtMs ASC")
    abstract fun observeAccounts(): Flow<List<LinkedAccountRow>>

    @Query("SELECT * FROM linked_account")
    abstract suspend fun accounts(): List<LinkedAccountRow>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    abstract suspend fun putAccount(row: LinkedAccountRow)

    @Query("DELETE FROM linked_account WHERE provider = :provider")
    abstract suspend fun deleteAccount(provider: String)

    // --- favorite characters ---
    @Query("SELECT * FROM favorite_character ORDER BY addedAtMs DESC")
    abstract fun observeCharacters(): Flow<List<FavoriteCharacterRow>>

    @Query("SELECT COUNT(*) FROM favorite_character WHERE id = :id")
    abstract fun observeCharacterCount(id: Int): Flow<Int>

    @Query("SELECT * FROM favorite_character WHERE id = :id")
    abstract suspend fun character(id: Int): FavoriteCharacterRow?

    @Query("SELECT * FROM favorite_character WHERE pendingRemote IS NOT NULL")
    abstract suspend fun pendingCharacters(): List<FavoriteCharacterRow>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    abstract suspend fun putCharacter(row: FavoriteCharacterRow)

    @Query("DELETE FROM favorite_character WHERE id = :id")
    abstract suspend fun deleteCharacter(id: Int)

    /** A favorite removed while offline is kept as a tombstone (pendingRemote = false) until AniList hears about it. */
    @Query("SELECT * FROM favorite_character WHERE pendingRemote IS NULL OR pendingRemote = 1 ORDER BY addedAtMs DESC")
    abstract fun observeVisibleCharacters(): Flow<List<FavoriteCharacterRow>>

    @Query("SELECT COUNT(*) FROM favorite_character WHERE id = :id AND (pendingRemote IS NULL OR pendingRemote = 1)")
    abstract fun observeVisibleCharacterCount(id: Int): Flow<Int>
}
