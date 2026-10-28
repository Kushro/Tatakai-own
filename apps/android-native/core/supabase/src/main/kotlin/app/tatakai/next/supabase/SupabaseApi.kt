package app.tatakai.next.supabase

import app.tatakai.next.model.UserSession
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** Raised when the server rejected the token and one refresh did not help: treat the user as signed out. */
class SessionExpiredException : Exception("Supabase session expired")

/** Raised when URL / anon key were not injected (TATAKAI_SUPABASE_*); the app keeps working local-only. */
class SupabaseNotConfiguredException : Exception("Supabase is not configured")

/** `public.watch_history` row, literal column names from contracts/supabase/m0-tables.md. */
@Serializable
data class RemoteWatchRow(
    @SerialName("anime_id") val animeId: String,
    @SerialName("anime_name") val animeName: String,
    @SerialName("anime_poster") val animePoster: String? = null,
    @SerialName("episode_id") val episodeId: String,
    @SerialName("episode_number") val episodeNumber: Int,
    @SerialName("progress_seconds") val progressSeconds: Int = 0,
    @SerialName("duration_seconds") val durationSeconds: Int? = null,
    val completed: Boolean = false,
    /** UTC ISO-8601, e.g. 2026-10-27T21:04:11.000Z */
    @SerialName("watched_at") val watchedAt: String,
    @SerialName("mal_id") val malId: Int? = null,
    @SerialName("anilist_id") val anilistId: Int? = null,
)

/** GoTrue email + password. Tokens never leave this module; only [UserSession] is exposed. */
interface SupabaseAuthSource {
    val session: StateFlow<UserSession?>
    suspend fun signIn(email: String, password: String): Result<Unit>
    suspend fun signOut()

    /** `profiles.is_banned` for the signed-in user (explicit columns only, never `*`). False when unknown. */
    suspend fun isBanned(): Boolean
}

/** `watch_history` access for the signed-in user. */
interface WatchHistoryRemote {
    /**
     * Upsert with `on_conflict=user_id,episode_id` + `Prefer: resolution=merge-duplicates`.
     * `mal_id` / `anilist_id` keys are omitted when null so an existing id is never overwritten with null.
     * @throws SessionExpiredException, SupabaseNotConfiguredException, or a network/HTTP exception.
     */
    suspend fun upsert(rows: List<RemoteWatchRow>)

    /** `select=*&user_id=eq.<uid>&order=watched_at.desc&limit=<limit>` */
    suspend fun recent(limit: Int = 30): List<RemoteWatchRow>
}
