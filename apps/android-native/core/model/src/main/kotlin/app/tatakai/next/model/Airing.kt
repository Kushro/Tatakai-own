// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.Serializable

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.1): airing calendar, followed shows ("Seguir") and
// local reminders. Desktop counterpart: tk-ui components/airing.rs (worker b). Signatures FINAL; bodies: worker r (:core:discover,
// `discoverModule`; the AniList `AiringSchedule` query lives in :core:metadata + core/data/MetadataRepositoryImpl.kt).

/** One scheduled episode (AniList `AiringSchedule` node). */
data class AiringEpisode(val media: Media, val episode: Int, val airingAtEpochSec: Long)

/** A series the user follows in the calendar (classic TrackShowButton). Independent of the AniList / MAL list. */
@Serializable
data class FollowedShow(
    val ref: MediaRef,
    val title: String,
    val poster: String? = null,
    val followedAtMs: Long,
    /** Local notification when an episode airs (`AppSettings.airingReminders` must be on too). */
    val remind: Boolean = true,
)

/**
 * Calendar + follow + reminders. Schedule = `MetadataRepository.airingSchedule` (AniList) cached per day; followed shows in a private
 * store of :core:discover; reminders through a periodic WorkManager worker that posts a local notification (channel `airing`) for every
 * followed episode that aired since the previous run. Implemented by worker r (`AiringRepositoryImpl`).
 */
interface AiringRepository {
    val followed: StateFlow<List<FollowedShow>>

    fun isFollowed(ref: MediaRef): Flow<Boolean>

    suspend fun follow(media: Media, remind: Boolean = true)

    suspend fun unfollow(ref: MediaRef)

    suspend fun setReminder(ref: MediaRef, enabled: Boolean)

    /** Episodes airing in `[fromEpochSec, toEpochSec)`, soonest first (the calendar week view groups them by day). */
    suspend fun schedule(fromEpochSec: Long, toEpochSec: Long): List<AiringEpisode>

    /** Followed / WATCHING titles that aired since [markVisited] was last called (Home row "Novedades de lo que sigues"). */
    suspend fun newSinceLastVisit(): List<AiringEpisode>

    suspend fun markVisited()
}
