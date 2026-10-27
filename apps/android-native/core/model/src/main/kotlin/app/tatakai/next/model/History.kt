package app.tatakai.next.model

import java.time.Instant

/**
 * Keys follow contracts/supabase/m0-tables.md (E9): [animeId] = String(anilistId) or "mal:<malId>",
 * [episodeId] = "<animeId>?ep=<episode>". [completed] when progress/duration >= 0.9.
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
