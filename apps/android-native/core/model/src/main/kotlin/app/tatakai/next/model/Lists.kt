// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.3): AniList / MAL lists, favorites and linked accounts.
// Desktop counterpart: tk-core/lists/** (worker g). Signatures are FINAL; the implementation is worker q's (:core:data, `dataModule`).
// JSON = camelCase, lowercase enum ids (desktop serde), so an export of one app imports into the other.

/** External list service. [id] is the stable key used in persistence, OAuth redirects (`tatakai://oauth/<id>`) and exports. */
@Serializable
enum class ListProvider(val id: String) {
    @SerialName("anilist") ANILIST("anilist"),
    @SerialName("mal") MAL("mal"),
}

/** Watch status, the union of AniList `MediaListStatus` and MAL `my_list_status.status` (MAL has no REPEATING: mapped to WATCHING + is_rewatching). */
@Serializable
enum class ListStatus(val id: String) {
    @SerialName("watching") WATCHING("watching"),
    @SerialName("planning") PLANNING("planning"),
    @SerialName("completed") COMPLETED("completed"),
    @SerialName("paused") PAUSED("paused"),
    @SerialName("dropped") DROPPED("dropped"),
    @SerialName("repeating") REPEATING("repeating"),
}

/**
 * One row of the user's list, merged across providers (the local copy is the source of truth; providers are synced to it).
 * [progress] = last episode watched (0 = none). [score] is 0..100 (AniList POINT_100; MAL 0..10 is multiplied by 10).
 */
@Serializable
data class ListEntry(
    val ref: MediaRef,
    val title: String,
    val poster: String?,
    val status: ListStatus,
    val progress: Int = 0,
    val totalEpisodes: Int? = null,
    val score: Int? = null,
    val favorite: Boolean = false,
    /** Local change time (epoch ms); the sync compares it with the provider's `updatedAt`. */
    val updatedAtMs: Long,
    val notes: String? = null,
    /** ISO-8601 dates (`2026-01-31`) when known. */
    val startedAt: String? = null,
    val completedAt: String? = null,
)

/** A provider account linked through OAuth. Tokens never leave :core:data (sealed under `noBackupFilesDir`, same pattern as `ExtensionServerStore`). */
data class LinkedAccount(
    val provider: ListProvider,
    val userId: String,
    val username: String,
    val avatar: String?,
    val linkedAtMs: Long,
    /** Access token expiry; null = the provider did not say (AniList tokens last a year). */
    val expiresAtMs: Long?,
)

/** The same title changed locally and on [provider] since the last sync, with different values. */
data class ListConflict(
    /** Stable id (`<provider>:<animeId>`), passed to [ListsRepository.resolveConflict]. */
    val id: String,
    val provider: ListProvider,
    val local: ListEntry,
    val remote: ListEntry,
)

enum class ConflictChoice { KEEP_LOCAL, KEEP_REMOTE }

data class ListSyncReport(val pushed: Int, val pulled: Int, val conflicts: Int, val failed: Int)

/**
 * Lists, favorites and linked accounts (desktop `tk-core/lists`, legacy `hooks/user/useWatchlist.ts` + `lib/listSync.ts`).
 * Local-first: every write lands in Room at once and is queued for the linked providers (offline queue, [pendingChanges]);
 * [syncNow] pushes the queue and pulls remote changes, leaving disagreements in [conflicts]. Without a linked account the
 * list still works locally. Implemented by worker q in :core:data (bound in `dataModule`); consumed by Library (q), the
 * `ListButton` of Details (q's composable, mounted by o) and Settings › Listas y cuentas (q).
 */
interface ListsRepository {
    val accounts: StateFlow<List<LinkedAccount>>

    /** Queued local changes not yet pushed to any provider (Settings › Cuentas shows the counter). */
    val pendingChanges: StateFlow<Int>

    /** Epoch ms of the last successful [syncNow]; null = never. */
    val lastSyncedAt: StateFlow<Long?>

    /** All entries, or only those with [status]; sorted by [ListEntry.updatedAtMs] desc. */
    fun entries(status: ListStatus? = null): Flow<List<ListEntry>>

    fun favorites(): Flow<List<ListEntry>>

    fun entry(ref: MediaRef): Flow<ListEntry?>

    /** Add / move [media] to [status]; null removes it from the list (favorite is kept). */
    suspend fun setStatus(media: Media, status: ListStatus?): Result<Unit>

    /** Sets the episode progress; moves PLANNING to WATCHING on the first episode and to COMPLETED when it reaches the total. */
    suspend fun setProgress(ref: MediaRef, episode: Int): Result<Unit>

    /** 0..100; null clears the score. */
    suspend fun setScore(ref: MediaRef, score: Int?): Result<Unit>

    suspend fun setFavorite(media: Media, favorite: Boolean): Result<Unit>

    /** Favorite characters (classic `character.favorite`): local, mirrored to AniList `ToggleFavourite(characterId)` when linked. */
    fun favoriteCharacters(): Flow<List<CharacterRef>>

    fun isCharacterFavorite(characterId: Int): Flow<Boolean>

    suspend fun setCharacterFavorite(character: CharacterRef, favorite: Boolean): Result<Unit>

    /**
     * Called by the history layer (worker q, inside :core:data) when an episode reaches `HistoryEntry.COMPLETED_RATIO` and
     * `AppSettings.autoListProgress` is on: advances progress for titles that are already on the list. Not for UI use.
     */
    suspend fun onEpisodeCompleted(ref: MediaRef, episode: Int) {}

    /**
     * Starts the OAuth flow: returns the authorization URL to open in a Custom Tab. The provider redirects to
     * `tatakai://oauth/<provider.id>` (manifest of :app), which MainActivity hands to `PendingOAuthRedirect`; the implementation
     * finishes it through [completeLink]. AniList: authorization code; MAL: authorization code + PKCE (plain).
     */
    suspend fun beginLink(provider: ListProvider): Result<String>

    /** Exchanges the redirect URI (`tatakai://oauth/<id>?code=…&state=…`) for tokens, stores them and pulls the account. */
    suspend fun completeLink(redirectUri: String): Result<LinkedAccount>

    suspend fun unlink(provider: ListProvider)

    /** Push the queue, pull remote lists, record conflicts. Without linked accounts: success with zeros. */
    suspend fun syncNow(): Result<ListSyncReport>

    fun conflicts(): Flow<List<ListConflict>>

    suspend fun resolveConflict(id: String, choice: ConflictChoice)

    /** JSON array of [ListEntry] (the desktop import/export format). */
    suspend fun exportJson(): String

    /** Merges an [exportJson] document (newest `updatedAtMs` wins); returns the number of entries imported or updated. */
    suspend fun importJson(json: String): Result<Int>

    // --- parity wave B (16 §3.7): the AniList token is the only credential of the social layer (t) and of the manga / novel readlist
    // sync (s). Implemented by the contract in ListsRepositoryImpl (reads the TokenSet of q, refreshing MAL when expired). ---
    /** A valid access token of a linked provider; null when not linked or expired without refresh. Never logged, never shown. */
    suspend fun accessToken(provider: ListProvider): String? = null
}
