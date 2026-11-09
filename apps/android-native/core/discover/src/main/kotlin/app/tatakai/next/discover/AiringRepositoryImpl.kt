// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.AiringRepository
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.FollowedShow
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import java.io.File

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r; checklist home.calendar.*, details.track-show,
 * home.followed-updates): [AiringRepository] over `MetadataRepository.airingSchedule` (AniList `Page.airingSchedules`, cached per
 * day in `storeDir/schedule-<yyyy-MM-dd>.json`, 6 h TTL), followed shows in `storeDir/followed.json`, last visit in `storeDir/visit`.
 * `newSinceLastVisit` = aired episodes (since the stamp) of the followed shows plus the WATCHING entries of [lists] when bound.
 * Reminders: [AiringReminderWorker] (periodic, 30 min) posts one notification per followed episode aired since its previous run and
 * `remind == true`, only when [remindersEnabled] (`AppSettings.airingReminders`).
 *
 * Stub: nothing is followed, the schedule is empty.
 */
class AiringRepositoryImpl(
    private val metadata: MetadataRepository,
    private val lists: ListsRepository? = null,
    private val storeDir: File,
    private val remindersEnabled: () -> Boolean = { true },
    private val clock: () -> Long = System::currentTimeMillis,
) : AiringRepository, AppDataWiper {
    private val _followed = MutableStateFlow<List<FollowedShow>>(emptyList())
    override val followed: StateFlow<List<FollowedShow>> = _followed.asStateFlow()

    override fun isFollowed(ref: MediaRef): Flow<Boolean> = _followed.map { list -> list.any { it.ref == ref } }

    override suspend fun follow(media: Media, remind: Boolean) {
        // wave B (r): persist to followed.json; stub keeps it in memory so the button toggles visibly.
        _followed.value = _followed.value.filterNot { it.ref == media.ref } +
            FollowedShow(ref = media.ref, title = media.title, poster = media.cover, followedAtMs = clock(), remind = remind)
    }

    override suspend fun unfollow(ref: MediaRef) {
        _followed.value = _followed.value.filterNot { it.ref == ref }
    }

    override suspend fun setReminder(ref: MediaRef, enabled: Boolean) {
        _followed.value = _followed.value.map { if (it.ref == ref) it.copy(remind = enabled) else it }
    }

    override suspend fun schedule(fromEpochSec: Long, toEpochSec: Long): List<AiringEpisode> = emptyList()

    override suspend fun newSinceLastVisit(): List<AiringEpisode> = emptyList()

    override suspend fun markVisited() {}

    /** "Restablecer la app": forgets followed shows, the visit stamp and the schedule cache. */
    override suspend fun wipe() {
        _followed.value = emptyList()
        storeDir.deleteRecursively()
    }

    companion object {
        const val DIR_NAME: String = "discover"

        /** Notification channel of the reminders (created by [AiringReminderWorker]). */
        const val CHANNEL_ID: String = "airing"
    }
}
