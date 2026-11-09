package app.tatakai.next.database

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module

@Database(entities = [HistoryRow::class, FeedCacheRow::class, SourceStatRow::class, PickedSourceRow::class], version = 2, exportSchema = false)
abstract class TatakaiDatabase : RoomDatabase() {
    abstract fun history(): HistoryDao
    abstract fun feedCache(): FeedCacheDao
    abstract fun sourceStats(): SourceStatsDao

    companion object {
        const val NAME = "tatakai.db"

        fun build(context: Context): TatakaiDatabase =
            Room.databaseBuilder(context.applicationContext, TatakaiDatabase::class.java, NAME)
                // v1: no migrations yet; history is re-fetched from the paired PC (Tatakai Link) when one exists.
                // v2 (android wave, 12 §4 a): + source_stats / picked_source. Still destructive (accepted, KNOWN-ISSUES): local
                // unsynced history, source reliabilities and manual picks are dropped on the upgrade.
                .fallbackToDestructiveMigration(dropAllTables = true)
                .build()
    }
}

/** TTL view over [FeedCacheDao]. [clock] is injectable for tests. */
class FeedCache(private val dao: FeedCacheDao, private val clock: () -> Long = System::currentTimeMillis) {
    /** Returns the cached JSON if it is younger than [ttlMs]. */
    suspend fun getFresh(key: String, ttlMs: Long): String? =
        dao.get(key)?.takeIf { clock() - it.fetchedAtMs <= ttlMs }?.json

    /** Returns the cached JSON regardless of age (used when offline or rate limited). */
    suspend fun getStale(key: String): String? = dao.get(key)?.json

    suspend fun put(key: String, json: String) = dao.put(FeedCacheRow(key, json, clock()))

    suspend fun purge(maxAgeMs: Long = 7L * 24 * 60 * 60 * 1000) = dao.purgeOlderThan(clock() - maxAgeMs)

    /** Number of cached responses and their size in bytes. */
    suspend fun stats(): Pair<Int, Long> = dao.count() to dao.totalBytes()

    suspend fun clear() = dao.clear()
}

val databaseModule: Module = module {
    single { TatakaiDatabase.build(androidContext()) }
    single { get<TatakaiDatabase>().history() }
    single { get<TatakaiDatabase>().feedCache() }
    single { get<TatakaiDatabase>().sourceStats() }
    single { FeedCache(get()) }
}
