package app.tatakai.next.database

import androidx.room.Entity
import androidx.room.Index
import androidx.room.PrimaryKey

/**
 * Local watch history, one row per episode (keys per contracts/supabase/m0-tables.md E9).
 * [syncedAtMs] is the [watchedAtMs] value last confirmed on the server; the row is dirty while it differs.
 */
@Entity(
    tableName = "history",
    indices = [Index("animeId"), Index("anilistId"), Index("watchedAtMs")],
)
data class HistoryRow(
    @PrimaryKey val episodeId: String,
    val animeId: String,
    val title: String,
    val poster: String?,
    val episode: Int,
    val progressSec: Int,
    val durationSec: Int?,
    val completed: Boolean,
    val watchedAtMs: Long,
    val anilistId: Int?,
    val malId: Int?,
    val syncedAtMs: Long? = null,
) {
    fun isDirty(): Boolean = syncedAtMs != watchedAtMs
}

/** Metadata response cache (JSON blob) with a fetch timestamp; TTL is applied by [FeedCache]. */
@Entity(tableName = "feed_cache")
data class FeedCacheRow(
    @PrimaryKey val key: String,
    val json: String,
    val fetchedAtMs: Long,
)

/** android wave (12 §3.1): reliability moving average of one `(extId, label)` source (desktop LocalStore `source_stats`). */
@Entity(tableName = "source_stats", primaryKeys = ["extId", "label"])
data class SourceStatRow(
    val extId: String,
    val label: String,
    val reliability: Float,
    val okCount: Int = 0,
    val failCount: Int = 0,
    val updatedAtMs: Long,
)

/** android wave (12 §3.1): the source the user picked manually for one episode ([key] = `sourceKey`). */
@Entity(tableName = "picked_source", primaryKeys = ["mediaId", "episode"])
data class PickedSourceRow(
    val mediaId: String,
    val episode: Int,
    val key: String,
    val updatedAtMs: Long,
)
