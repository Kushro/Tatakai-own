package app.tatakai.next.metadata

import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.ImageMatch
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.CancellationException
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

    fun isCoolingDown(id: String): Boolean = (downUntil[id] ?: 0L) > clock()

    fun resetHealth() = downUntil.clear()

    suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): Routed<FeedPage> =
        route(Capability.FEED, kind, { it.items.isEmpty() }) { it.feed(kind, feed, page) }

    suspend fun search(kind: MediaKind, query: String, page: Int): Routed<FeedPage> =
        route(Capability.SEARCH, kind, { it.items.isEmpty() }) { it.search(kind, query, page) }

    suspend fun media(kind: MediaKind, ref: MediaRef): Routed<Media?> =
        route(Capability.MEDIA, kind, { it == null }) { it.media(kind, ref) }

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
                downUntil.remove(p.id)
                if (isEmpty(value)) {
                    if (empty == null) empty = Routed(value, p.id)
                    continue
                }
                return Routed(value, p.id)
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                if (e is ProviderException && e.tripsCooldown) downUntil[p.id] = clock() + cooldownMs
                errors += p.id to e
            }
        }
        empty?.let { return it }
        throw NoMetadataProviderException(cap, errors)
    }

    companion object {
        const val DEFAULT_COOLDOWN_MS = 60_000L
    }
}
