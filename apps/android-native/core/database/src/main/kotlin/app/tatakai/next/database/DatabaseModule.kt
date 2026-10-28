package app.tatakai.next.database

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module

@Database(entities = [HistoryRow::class, FeedCacheRow::class], version = 1, exportSchema = false)
abstract class TatakaiDatabase : RoomDatabase() {
    abstract fun history(): HistoryDao
    abstract fun feedCache(): FeedCacheDao

    companion object {
        const val NAME = "tatakai.db"

        fun build(context: Context): TatakaiDatabase =
            Room.databaseBuilder(context.applicationContext, TatakaiDatabase::class.java, NAME)
                // v1: no migrations yet; history is re-fetched from Supabase when signed in.
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
}

val databaseModule: Module = module {
    single { TatakaiDatabase.build(androidContext()) }
    single { get<TatakaiDatabase>().history() }
    single { get<TatakaiDatabase>().feedCache() }
    single { FeedCache(get()) }
}
