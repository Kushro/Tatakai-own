package app.tatakai.next.metadata

import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.ImageMatch
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.ProviderStatus
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import java.util.concurrent.ConcurrentHashMap

/**
 * Calls providers in the user's priority order, skipping ones that don't support the call or are cooling down,
 * and falls back to the next provider on error or empty result. Mirrors the legacy `src/lib/metadata/router.ts`.
 *
 * @param priority ids of the enabled providers, best first (AppSettings.providerPriority). Ids not listed are disabled.
 * @param cooldownMs how long a provider is skipped after a transport/HTTP failure (contract: 60 s).
 */
class MetadataRouter(
    private val providers: List<MetadataProvider>,
    private val priority: () -> List<String>,
    private val clock: () -> Long = System::currentTimeMillis,
    private val cooldownMs: Long = DEFAULT_COOLDOWN_MS,
    /** Parity wave A: trace.moe scene search (not a metadata provider: no priority, no cool-down). */
    private val imageSearch: TraceMoeClient? = null,
) {
    private val downUntil = ConcurrentHashMap<String, Long>()

    data class Routed<T>(val value: T, val provider: String)

    /**
     * Parity wave B (16 §3.1, worker r, checklist settings.metadata.provider-status): last outcome of every provider called in this
     * process (`enabled` is filled at read time by [statusSnapshot] / `MetadataRepositoryImpl.providerStatus`).
     */
    private val _status = MutableStateFlow<Map<String, ProviderStatus>>(emptyMap())
    val status: StateFlow<Map<String, ProviderStatus>> = _status.asStateFlow()

    /** Ids of the enabled providers, best first (the user's `providerPriority`). */
    fun enabledIds(): List<String> = priority()

    /** One row per known provider id: enabled flag from the priority, last result and cooldown from this router. */
    fun statusSnapshot(ids: List<String>): List<ProviderStatus> {
        val enabled = priority()
        val now = clock()
        val known = _status.value
        return ids.map { id ->
            val s = known[id] ?: ProviderStatus(id = id, enabled = false)
            s.copy(enabled = id in enabled, cooldownUntilMs = downUntil[id]?.takeIf { it > now })
        }
    }

    fun isCoolingDown(id: String): Boolean = (downUntil[id] ?: 0L) > clock()

    fun resetHealth() {
        downUntil.clear()
        _status.update { m -> m.mapValues { it.value.copy(cooldownUntilMs = null) } }
    }

    private fun recordOk(id: String) {
        downUntil.remove(id)
        val now = clock()
        _status.update { m -> m + (id to (m[id] ?: ProviderStatus(id, true)).copy(lastOkAtMs = now, lastError = null, cooldownUntilMs = null)) }
    }

    private fun recordError(id: String, e: Exception) {
        val until = if (e is ProviderException && e.tripsCooldown) clock() + cooldownMs else null
        if (until != null) downUntil[id] = until
        val msg = when (e) {
            is ProviderException -> if (e.status == 0) "offline" else "HTTP ${e.status}"
            else -> e.message?.take(120) ?: e::class.simpleName.orEmpty()
        }
        _status.update { m -> m + (id to (m[id] ?: ProviderStatus(id, true)).copy(lastError = msg, cooldownUntilMs = until)) }
    }

    suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): Routed<FeedPage> =
        route(Capability.FEED, kind, { it.items.isEmpty() }) { it.feed(kind, feed, page) }

    suspend fun search(kind: MediaKind, query: String, page: Int): Routed<FeedPage> =
        route(Capability.SEARCH, kind, { it.items.isEmpty() }) { it.search(kind, query, page) }

    /**
     * The first enabled provider that knows the title answers; then (parity wave B, checklist settings.metadata.extra-providers) the
     * other enabled MEDIA providers fill what it lacks (score, synopsis, cover, banner, episodes, genres, alternative titles), in priority
     * order, using the MAL id the first answer carried (MAL / Shikimori / Tenrai map by `idMal`). Failures of the fillers never fail
     * the call (they only update the provider status / cooldown).
     */
    suspend fun media(kind: MediaKind, ref: MediaRef): Routed<Media?> {
        val first = route(Capability.MEDIA, kind, { it == null }) { it.media(kind, ref) }
        var value = first.value ?: return first
        if (!value.hasGaps()) return first
        val malRef = ref.copy(malId = value.ref.malId ?: ref.malId, anilistId = value.ref.anilistId ?: ref.anilistId)
        if (malRef.malId == null && malRef.anilistId == null) return first
        val fillers = candidates(Capability.MEDIA, kind, includeCoolingDown = false).filter { it.id != first.provider }
        for (p in fillers.take(MAX_FILLERS)) {
            try {
                val extra = p.media(kind, malRef)
                recordOk(p.id)
                if (extra != null) value = fillGaps(value, extra)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                recordError(p.id, e)
            }
            if (!value.hasGaps()) break
        }
        return Routed(value, first.provider)
    }

    suspend fun episodes(ref: MediaRef): Routed<List<Episode>> =
        route(Capability.EPISODES, MediaKind.ANIME, { it.isEmpty() }) { it.episodes(ref) }

    // --- parity wave A (docs/redesign/15 §3.1, worker o) ---

    /**
     * Routes a [DiscoverProvider] call (advanced search, details extras, characters, trending windows) through the enabled providers
     * with [Capability.DISCOVER] in priority order. Throws [NoMetadataProviderException] with no errors when none is enabled (callers
     * treat that as "not available"), with the errors when every one failed.
     */
    suspend fun <T> discover(kind: MediaKind = MediaKind.ANIME, isEmpty: (T) -> Boolean, call: suspend (DiscoverProvider) -> T): Routed<T> =
        route(Capability.DISCOVER, kind, isEmpty) { p -> call(p as DiscoverProvider) }

    /** Parity wave B (16 §3.1, worker r): AniList airing schedule through the enabled DISCOVER providers that implement [AiringProvider]. */
    suspend fun airingSchedule(fromEpochSec: Long, toEpochSec: Long, page: Int): Routed<Pair<List<AiringEpisode>, Boolean>> =
        route(Capability.DISCOVER, MediaKind.ANIME, { it.first.isEmpty() }) { p ->
            (p as? AiringProvider)?.airingSchedule(fromEpochSec, toEpochSec, page) ?: (emptyList<AiringEpisode>() to false)
        }

    val supportsImageSearch: Boolean get() = imageSearch != null

    suspend fun searchByImage(image: ByteArray, mimeType: String): List<ImageMatch> =
        imageSearch?.search(image, mimeType) ?: throw NoMetadataProviderException(Capability.DISCOVER, emptyList())

    private fun candidates(cap: Capability, kind: MediaKind, includeCoolingDown: Boolean): List<MetadataProvider> =
        priority().mapNotNull { id -> providers.firstOrNull { it.id == id } }
            .distinctBy { it.id }
            .filter { cap in it.capabilities && kind in it.kinds }
            .filter { cap != Capability.DISCOVER || it is DiscoverProvider }
            .filter { includeCoolingDown || !isCoolingDown(it.id) }

    private suspend fun <T> route(
        cap: Capability,
        kind: MediaKind,
        isEmpty: (T) -> Boolean,
        call: suspend (MetadataProvider) -> T,
    ): Routed<T> {
        var list = candidates(cap, kind, includeCoolingDown = false)
        // Last resort: when everything is cooling down, retry anyway rather than failing instantly.
        if (list.isEmpty()) list = candidates(cap, kind, includeCoolingDown = true)
        val errors = mutableListOf<Pair<String, Throwable>>()
        var empty: Routed<T>? = null
        for (p in list) {
            try {
                val value = call(p)
                recordOk(p.id)
                if (isEmpty(value)) {
                    if (empty == null) empty = Routed(value, p.id)
                    continue
                }
                return Routed(value, p.id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                recordError(p.id, e)
                errors += p.id to e
            }
        }
        empty?.let { return it }
        throw NoMetadataProviderException(cap, errors)
    }

    companion object {
        const val DEFAULT_COOLDOWN_MS = 60_000L
        private const val MAX_FILLERS = 3

        internal fun Media.hasGaps(): Boolean =
            score == null || synopsis.isNullOrBlank() || cover == null || episodes == null || genres.isEmpty()

        /** [base] keeps everything it has; [extra] only fills nulls / empties (titles: missing keys). */
        internal fun fillGaps(base: Media, extra: Media): Media = base.copy(
            ref = base.ref.copy(
                anilistId = base.ref.anilistId ?: extra.ref.anilistId,
                malId = base.ref.malId ?: extra.ref.malId,
                kitsuId = base.ref.kitsuId ?: extra.ref.kitsuId,
            ),
            titles = extra.titles + base.titles,
            cover = base.cover ?: extra.cover,
            banner = base.banner ?: extra.banner,
            color = base.color ?: extra.color,
            synopsis = base.synopsis?.takeIf { it.isNotBlank() } ?: extra.synopsis,
            genres = base.genres.ifEmpty { extra.genres },
            episodes = base.episodes ?: extra.episodes,
            status = base.status ?: extra.status,
            season = base.season ?: extra.season,
            year = base.year ?: extra.year,
            score = base.score ?: extra.score,
        )
    }
}
