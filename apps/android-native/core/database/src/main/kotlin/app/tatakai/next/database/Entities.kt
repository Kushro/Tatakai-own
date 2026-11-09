package app.tatakai.next.database

import androidx.room.Entity
import androidx.room.Index
import androidx.room.PrimaryKey

/**
 * Local watch history, one row per episode (legacy key convention of contracts/supabase/m0-tables.md E9, shared with Tatakai Link /v1/history).
 * [syncedAtMs] is the [watchedAtMs] value last confirmed by the paired PC; the row is dirty while it differs.
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

// --- parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.3, worker q): AniList / MAL lists, local first ----------------

/**
 * One title of the user's list (the local copy is the source of truth). [key] = `HistoryEntry.animeIdOf(anilistId, malId)` (`"21"` or
 * `"mal:5114"`). [status] is `ListStatus.id`; null = not on the list (a favorite-only row). [score] 0..100.
 * [remoteJson] keeps, per provider, the last values both sides agreed on (`{"anilist":{...},"mal":{...}}`): the conflict base.
 */
@Entity(tableName = "list_entry", indices = [Index("status"), Index("favorite"), Index("updatedAtMs")])
data class ListEntryRow(
    @PrimaryKey val key: String,
    val anilistId: Int?,
    val malId: Int?,
    val title: String,
    val poster: String?,
    val status: String?,
    val progress: Int = 0,
    val totalEpisodes: Int? = null,
    val score: Int? = null,
    val favorite: Boolean = false,
    val updatedAtMs: Long,
    val notes: String? = null,
    val startedAt: String? = null,
    val completedAt: String? = null,
    val remoteJson: String? = null,
)

/**
 * Offline queue: one pending push per (provider, title). [op] = `"save"` (status / progress / score from the current row), `"delete"`,
 * `"favorite"` (AniList ToggleFavourite of the title; [favorite] = wanted value). Coalesced: a newer change replaces the older one.
 */
@Entity(tableName = "list_change", primaryKeys = ["provider", "key", "op"])
data class ListChangeRow(
    val provider: String,
    val key: String,
    val op: String,
    val favorite: Boolean? = null,
    val createdAtMs: Long,
    val attempts: Int = 0,
    val lastError: String? = null,
)

/** A title that changed on both sides with different values; [localJson] / [remoteJson] are `ListEntry` JSON. Id `<provider>:<key>`. */
@Entity(tableName = "list_conflict")
data class ListConflictRow(
    @PrimaryKey val id: String,
    val provider: String,
    val key: String,
    val localJson: String,
    val remoteJson: String,
    val createdAtMs: Long,
)

/** A linked list provider (no tokens here: they live in `SecretStore`). */
@Entity(tableName = "linked_account")
data class LinkedAccountRow(
    @PrimaryKey val provider: String,
    val userId: String,
    val username: String,
    val avatar: String?,
    val linkedAtMs: Long,
    val expiresAtMs: Long?,
    /** AniList `mediaListOptions.scoreFormat` (POINT_100, POINT_10_DECIMAL, POINT_10, POINT_5, POINT_3) or "mal" (0..10). */
    val scoreFormat: String? = null,
    val lastSyncedAtMs: Long? = null,
)

/** Favorite character (classic `character.favorite`), mirrored to AniList `ToggleFavourite(characterId)` when linked. */
@Entity(tableName = "favorite_character")
data class FavoriteCharacterRow(
    @PrimaryKey val id: Int,
    val name: String,
    val image: String?,
    val addedAtMs: Long,
    /** null = in sync; true / false = wanted AniList state not pushed yet. */
    val pendingRemote: Boolean? = null,
)
