// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.AiringRepository
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.FollowedShow
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.PER_PAGE_AIRING
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import java.io.File

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r; checklist home.calendar.*, details.track-show,
 * home.followed-updates): [AiringRepository] over `MetadataRepository.airingSchedule` (AniList `Page.airingSchedules`; the response
 * cache lives in `MetadataRepositoryImpl`, plus a 10 min in-memory window cache here), followed shows in `storeDir/followed.json`, the
 * last visit and the last reminder run in `storeDir/stamps.json`. `newSinceLastVisit` = aired episodes (since the visit stamp, at most
 * [MAX_LOOKBACK_SEC] back) of the followed shows plus the WATCHING entries of [lists] when bound. Reminders: [dueReminders] is called by
 * [AiringReminderWorker] (periodic, 30 min) and returns the followed episodes with `remind == true` aired since its previous run, only
 * while [remindersEnabled] (`AppSettings.airingReminders`).
 */
class AiringRepositoryImpl(
    private val metadata: MetadataRepository,
    private val lists: ListsRepository? = null,
    private val storeDir: File,
    private val remindersEnabled: () -> Boolean = { true },
    private val clock: () -> Long = System::currentTimeMillis,
) : AiringRepository, AppDataWiper {
    @Serializable
    internal data class Stamps(val visitedAtSec: Long? = null, val remindedUpToSec: Long? = null)

    private val followedStore = JsonFileStore(File(storeDir, "followed.json"), ListSerializer(FollowedShow.serializer())) { emptyList() }
    private val stampStore = JsonFileStore(File(storeDir, "stamps.json"), Stamps.serializer()) { Stamps() }
    private val lock = Mutex()

    private val _followed = MutableStateFlow(followedStore.readBlocking())
    override val followed: StateFlow<List<FollowedShow>> = _followed.asStateFlow()

    private var windowCache: Triple<Pair<Long, Long>, Long, List<AiringEpisode>>? = null

    private fun nowSec(): Long = clock() / 1000

    override fun isFollowed(ref: MediaRef): Flow<Boolean> = _followed.map { list -> list.any { sameTitle(it.ref, ref) } }.distinctUntilChanged()

    override suspend fun follow(media: Media, remind: Boolean): Unit = lock.withLock {
        val next = _followed.value.filterNot { sameTitle(it.ref, media.ref) } +
            FollowedShow(ref = media.ref, title = media.title, poster = media.cover, followedAtMs = clock(), remind = remind)
        _followed.value = next
        followedStore.write(next)
    }

    override suspend fun unfollow(ref: MediaRef): Unit = lock.withLock {
        val next = _followed.value.filterNot { sameTitle(it.ref, ref) }
        _followed.value = next
        followedStore.write(next)
    }

    override suspend fun setReminder(ref: MediaRef, enabled: Boolean): Unit = lock.withLock {
        val next = _followed.value.map { if (sameTitle(it.ref, ref)) it.copy(remind = enabled) else it }
        _followed.value = next
        followedStore.write(next)
    }

    override suspend fun schedule(fromEpochSec: Long, toEpochSec: Long): List<AiringEpisode> {
        if (toEpochSec <= fromEpochSec) return emptyList()
        val key = fromEpochSec to toEpochSec
        windowCache?.let { (k, at, list) -> if (k == key && clock() - at < WINDOW_CACHE_MS) return list }
        val out = mutableListOf<AiringEpisode>()
        for (page in 1..MAX_PAGES) {
            val chunk = metadata.airingSchedule(fromEpochSec, toEpochSec, page)
            out += chunk
            if (chunk.size < PER_PAGE_AIRING) break
        }
        val result = AiringLogic.dedupe(out)
        windowCache = Triple(key, clock(), result)
        return result
    }

    /** Refs of the shows whose new episodes matter: followed + WATCHING / REPEATING of the list. */
    private suspend fun trackedRefs(): List<MediaRef> {
        val watching = try {
            lists?.entries(null)?.first().orEmpty().filter { it.status == ListStatus.WATCHING || it.status == ListStatus.REPEATING }.map { it.ref }
        } catch (e: CancellationException) {
            throw e
        } catch (_: Exception) {
            emptyList()
        }
        return _followed.value.map { it.ref } + watching
    }

    override suspend fun newSinceLastVisit(): List<AiringEpisode> {
        val now = nowSec()
        val since = AiringLogic.lookbackStart(stampStore.read().visitedAtSec, now, MAX_LOOKBACK_SEC, DEFAULT_LOOKBACK_SEC)
        val tracked = trackedRefs()
        if (tracked.isEmpty()) return emptyList()
        return AiringLogic.forTracked(schedule(since, now + 1), tracked).sortedByDescending { it.airingAtEpochSec }
    }

    override suspend fun markVisited(): Unit = lock.withLock {
        stampStore.write(stampStore.read().copy(visitedAtSec = nowSec()))
    }

    /**
     * Followed episodes (remind on) that aired after the previous call; advances the stamp. Empty when reminders are off (the stamp still
     * moves so turning them back on does not replay old episodes).
     */
    suspend fun dueReminders(): List<AiringEpisode> {
        val now = nowSec()
        val stamps = stampStore.read()
        val from = stamps.remindedUpToSec?.coerceAtLeast(now - MAX_LOOKBACK_SEC) ?: (now - FIRST_RUN_LOOKBACK_SEC)
        val reminded = _followed.value.filter { it.remind }.map { it.ref }
        val due = if (!remindersEnabled() || reminded.isEmpty()) emptyList() else AiringLogic.forTracked(schedule(from, now + 1), reminded)
        lock.withLock { stampStore.write(stampStore.read().copy(remindedUpToSec = now + 1)) }
        return due
    }

    /** "Restablecer la app": forgets followed shows, the stamps and the window cache. */
    override suspend fun wipe() {
        lock.withLock {
            _followed.value = emptyList()
            windowCache = null
            storeDir.deleteRecursively()
        }
    }

    companion object {
        const val DIR_NAME: String = "discover"

        /** Notification channel of the reminders (created by [AiringReminderWorker]). */
        const val CHANNEL_ID: String = "airing"

        private const val MAX_PAGES = 6
        private const val WINDOW_CACHE_MS = 10L * 60_000
        internal const val MAX_LOOKBACK_SEC = 14L * 86_400
        internal const val DEFAULT_LOOKBACK_SEC = 7L * 86_400
        private const val FIRST_RUN_LOOKBACK_SEC = 30L * 60

        internal fun sameTitle(a: MediaRef, b: MediaRef): Boolean =
            (a.anilistId != null && a.anilistId == b.anilistId) || (a.malId != null && a.malId == b.malId) || (a.kitsuId != null && a.kitsuId == b.kitsuId)
    }
}

/** Pure helpers of the calendar (JVM-tested). */
internal object AiringLogic {
    fun dedupe(list: List<AiringEpisode>): List<AiringEpisode> =
        list.distinctBy { "${it.media.ref.anilistId}:${it.media.ref.malId}:${it.episode}" }.sortedBy { it.airingAtEpochSec }

    fun forTracked(schedule: List<AiringEpisode>, tracked: List<MediaRef>): List<AiringEpisode> =
        schedule.filter { ep -> tracked.any { AiringRepositoryImpl.sameTitle(it, ep.media.ref) } }

    /** Start of the "since last visit" window: the stamp, never older than [maxBack]; [defaultBack] without a stamp. */
    fun lookbackStart(visitedAtSec: Long?, nowSec: Long, maxBack: Long, defaultBack: Long): Long =
        (visitedAtSec ?: (nowSec - defaultBack)).coerceIn(nowSec - maxBack, nowSec)

    /** Local-day buckets of a week view: day index 0..6 from [weekStartSec] (`ZoneOffset` given in seconds). */
    fun dayIndex(airingAtSec: Long, weekStartSec: Long): Int = ((airingAtSec - weekStartSec) / 86_400).toInt()
}
