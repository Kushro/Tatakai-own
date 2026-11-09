// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AtlasPoint
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.RecFeedback
import app.tatakai.next.model.RecommendationItem
import app.tatakai.next.model.RecsRepository
import app.tatakai.next.model.TasteProfile
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.async
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.builtins.ListSerializer
import java.io.File
import java.time.Year

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r; checklist home.recs.*, home.because-you-watched):
 * [RecsRepository] computed on the device (port of the classic `src/core/recommendations/*`, pure part in [RecsEngine]). Signals: the
 * list entries (status + score, `ListsRepository`) and the watch history (completed episodes = positive signal), the most recent
 * [MAX_SIGNALS] titles; their genres / year / score from `MetadataRepository.media` and studios from `details` (top [MAX_STUDIO_LOOKUPS]).
 * Candidates: AniList `recommendations(ref)` of the [MAX_SEEDS] strongest titles ("Parecida a X") plus the Trending / Popular / Top
 * feeds. Dismissed refs persist in `storeDir/dismissed.json`; the last computation is kept [CACHE_MS] in memory.
 */
class RecsRepositoryImpl(
    private val metadata: MetadataRepository,
    private val history: HistoryRepository,
    private val lists: ListsRepository? = null,
    private val storeDir: File,
    private val clock: () -> Long = System::currentTimeMillis,
) : RecsRepository, AppDataWiper {
    private val dismissedStore = JsonFileStore(File(storeDir, "dismissed.json"), ListSerializer(MediaRef.serializer())) { emptyList() }
    private val dismissed = MutableStateFlow(dismissedStore.readBlocking())
    private val lock = Mutex()

    private data class Computed(
        val atMs: Long,
        val profile: TasteProfile?,
        val seen: List<Media>,
        val candidates: List<RecsEngine.Candidate>,
        val seenKeys: Set<String>,
    )

    private var cache: Computed? = null

    private suspend fun <T> safe(block: suspend () -> T): T? = try {
        block()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }

    private suspend fun signals(): Pair<List<RecsEngine.Signal>, Set<String>> = coroutineScope {
        data class Raw(val ref: MediaRef, val affinity: Float, val score: Int?)
        val raw = LinkedHashMap<String, Raw>()
        val entries = safe { lists?.entries(null)?.first() }.orEmpty().sortedByDescending { it.updatedAtMs }
        for (e in entries) {
            val key = RecsEngine.keyOf(e.ref) ?: continue
            raw[key] = Raw(e.ref, RecsEngine.affinityOf(e.status, e.score, completed = e.status == app.tatakai.next.model.ListStatus.COMPLETED), e.score)
        }
        val watched = safe { history.continueWatching(MAX_SIGNALS * 2).first() }.orEmpty()
        for (h in watched) {
            val ref = MediaRef(anilistId = h.anilistId, malId = h.malId)
            val key = RecsEngine.keyOf(ref) ?: continue
            if (key !in raw) raw[key] = Raw(ref, RecsEngine.affinityOf(null, null, completed = h.completed || h.episode > 1), null)
        }
        val seenKeys = raw.keys.toSet()
        val picked = raw.values.take(MAX_SIGNALS)
        val medias = picked.map { r -> async { safe { metadata.media(MediaKind.ANIME, r.ref) } } }.awaitAll()
        val withMedia = picked.zip(medias).mapNotNull { (r, m) -> m?.let { RecsEngine.Signal(it, r.affinity, r.score) } }
        val studioFor = withMedia.sortedByDescending { it.affinity }.take(MAX_STUDIO_LOOKUPS)
            .map { s -> async { s.media.ref to (safe { metadata.details(s.media.ref) }?.studios.orEmpty()) } }.awaitAll().toMap()
        withMedia.map { s -> s.copy(studios = studioFor[s.media.ref].orEmpty()) } to seenKeys
    }

    private suspend fun compute(force: Boolean = false): Computed = lock.withLock {
        cache?.takeIf { !force && clock() - it.atMs < CACHE_MS }?.let { return@withLock it }
        val (signals, seenKeys) = signals()
        val profile = RecsEngine.profile(signals)
        val candidates: List<RecsEngine.Candidate> = if (profile == null) emptyList() else coroutineScope {
            val seeds = signals.filter { it.affinity > 0f }.sortedByDescending { it.affinity }.take(MAX_SEEDS)
            val similar = seeds.map { s ->
                async { safe { metadata.recommendations(s.media.ref) }.orEmpty().map { RecsEngine.Candidate(it.media, similarTo = s.media.title) } }
            }
            val feeds = listOf(FeedKind.TRENDING, FeedKind.POPULAR, FeedKind.TOP_RATED, FeedKind.SEASONAL).map { f ->
                async { safe { metadata.feed(MediaKind.ANIME, f, 1) }?.items.orEmpty().map { RecsEngine.Candidate(it) } }
            }
            (similar + feeds).awaitAll().flatten()
        }
        Computed(clock(), profile, signals.map { it.media }, candidates, seenKeys).also { cache = it }
    }

    override suspend fun profile(): TasteProfile? = compute().profile

    override suspend fun forYou(limit: Int): List<RecommendationItem> {
        val c = compute()
        val profile = c.profile ?: return emptyList()
        val dismissedKeys = dismissed.value.mapNotNull(RecsEngine::keyOf).toSet()
        return RecsEngine.rank(c.candidates, profile, c.seenKeys, dismissedKeys, limit, Year.now().value)
    }

    override suspend fun feedback(ref: MediaRef, feedback: RecFeedback) {
        if (feedback != RecFeedback.NOT_INTERESTED) return
        lock.withLock {
            if (dismissed.value.none { RecsEngine.keyOf(it) == RecsEngine.keyOf(ref) }) {
                dismissed.value = dismissed.value + ref
                dismissedStore.write(dismissed.value)
            }
        }
    }

    override fun dismissed(): Flow<List<MediaRef>> = dismissed.asStateFlow()

    override suspend fun undoDismiss(ref: MediaRef) {
        lock.withLock {
            dismissed.value = dismissed.value.filterNot { RecsEngine.keyOf(it) == RecsEngine.keyOf(ref) }
            dismissedStore.write(dismissed.value)
        }
    }

    override suspend fun atlas(): List<AtlasPoint> {
        val c = compute()
        val recs = forYou(ATLAS_RECS).map { it.media }
        return RecsEngine.atlas(c.seen, recs)
    }

    override suspend fun wipe() {
        lock.withLock {
            dismissed.value = emptyList()
            cache = null
            storeDir.deleteRecursively()
        }
    }

    companion object {
        const val DIR_NAME: String = "recs"
        private const val MAX_SIGNALS = 30
        private const val MAX_STUDIO_LOOKUPS = 8
        private const val MAX_SEEDS = 5
        private const val ATLAS_RECS = 40
        private const val CACHE_MS = 30L * 60_000
    }
}
