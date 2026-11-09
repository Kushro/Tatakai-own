package app.tatakai.next.model

import java.time.Instant

/**
 * Keys keep the legacy `watch_history` shape (contracts/supabase/m0-tables.md E9, now only a key convention shared with desktop and the
 * Tatakai Link `/v1/history` rows): [animeId] = String(anilistId) or "mal:<malId>", [episodeId] = "<animeId>?ep=<episode>".
 * [completed] when progress/duration >= 0.9. [progressSec] is the exact resume position that syncs between the user's devices.
 */
data class HistoryEntry(
    val animeId: String,
    val title: String,
    val poster: String?,
    val episodeId: String,
    val episode: Int,
    val progressSec: Int,
    val durationSec: Int?,
    val completed: Boolean,
    val watchedAt: Instant,
    val anilistId: Int?,
    val malId: Int?,
) {
    companion object {
        const val COMPLETED_RATIO = 0.9

        fun animeIdOf(anilistId: Int?, malId: Int?): String? = when {
            anilistId != null && anilistId > 0 -> anilistId.toString()
            malId != null && malId > 0 -> "mal:$malId"
            else -> null
        }

        fun episodeIdOf(animeId: String, episode: Int): String = "$animeId?ep=$episode"

        fun isCompleted(progressSec: Int, durationSec: Int?, alreadyCompleted: Boolean = false): Boolean =
            alreadyCompleted || (durationSec != null && durationSec > 0 && progressSec.toDouble() / durationSec >= COMPLETED_RATIO)
    }
}
