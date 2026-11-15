package app.tatakai.next.data

import app.tatakai.next.database.FeedCache
import app.tatakai.next.metadata.DiscoverProvider
import app.tatakai.next.metadata.MetadataRouter
import app.tatakai.next.metadata.NoMetadataProviderException
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppLog
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.AppException
import app.tatakai.next.model.Character
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.CharactersPage
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.ImageMatch
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaDetails
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.MediaSearchQuery
import app.tatakai.next.model.MediaTag
import app.tatakai.next.model.MetadataCacheStats
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.ProviderStatus
import app.tatakai.next.model.Recommendation
import app.tatakai.next.model.StaffEdge
import app.tatakai.next.model.searchTitles
import app.tatakai.next.model.withAltTitles
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.flow
import kotlinx.serialization.KSerializer
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json

/**
 * Metadata through [MetadataRouter] with a Room-backed response cache (TTL). When every provider fails, a stale
 * cached copy is served instead of an error; with nothing cached an [AppException] carrying an [AppError] is thrown.
 */
class MetadataRepositoryImpl(
    private val router: MetadataRouter,
    private val cache: FeedCache,
    /** android wave: `AppSettings.metadataCacheTtlHours` (1..168) for details and episode lists; feeds keep [FEED_TTL_MS]. */
    private val ttlHours: () -> Int = { DEFAULT_TTL_HOURS },
) : MetadataRepository {
    private val json = Json { ignoreUnknownKeys = true; encodeDefaults = true }

    @Serializable
    private data class CachedPage(val items: List<Media>, val page: Int, val hasNext: Boolean)

    override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
        val key = "feed:$kind:$feed:$page"
        cache.getFresh(key, FEED_TTL_MS)?.let { raw -> decodePage(raw)?.let { return it } }
        return try {
            router.feed(kind, feed, page).value.also { cache.put(key, encodePage(it)) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            cache.getStale(key)?.let { decodePage(it) } ?: throw e.toAppException()
        }
    }

    override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage {
        if (query.isBlank()) return FeedPage(emptyList(), page, false)
        return try {
            router.search(kind, query, page).value
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            throw e.toAppException()
        }
    }

    /**
     * One cached `media:` row. [degraded] (BUG-143): a fallback provider answered because the primary (AniList) did not; such a row
     * is fresh for [FALLBACK_TTL_MS] only, so the next open asks the primary again. Rows written before BUG-143 are a bare [Media].
     */
    @Serializable
    private data class CachedMedia(val media: Media, val degraded: Boolean = false)

    override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
        val key = "media:$kind:${ref.anilistId}:${ref.malId}:${ref.kitsuId}"
        cache.getFresh(key, detailTtlMs())?.let { raw ->
            decodeMediaRow(raw, kind)?.let { row -> if (!row.degraded) return row.media }
        }
        cache.getFresh(key, FALLBACK_TTL_MS)?.let { raw -> decodeMediaRow(raw, kind)?.let { return it.media } }
        return try {
            val routed = router.media(kind, ref)
            val value = routed.value ?: return null
            val fallbackFrom = routed.fallbackFrom
            if (fallbackFrom == null) {
                value.also { cache.put(key, encodeMediaRow(CachedMedia(it))) }
            } else {
                fallbackMedia(key, kind, ref, value, routed.provider, fallbackFrom)
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            cache.getStale(key)?.let { decodeMediaRow(it, kind)?.media } ?: throw e.toAppException()
        }
    }

    /**
     * BUG-143: a fallback answer ([fallbackFrom] = why the primary did not answer) never replaces a complete row: that one is served
     * (same title, cover and title order) with the fallback's titles appended as [Media.altTitles], and left untouched so it is
     * retried once stale. Without a complete row the fallback is cached as degraded ([FALLBACK_TTL_MS]) with every title known
     * before (an older degraded row, AniList's synonyms from a cached `details:` row) kept as alt titles.
     */
    private suspend fun fallbackMedia(key: String, kind: MediaKind, ref: MediaRef, value: Media, provider: String, fallbackFrom: String): Media {
        val label = MetadataRouter.refLabel(ref)
        val previous = cache.getStale(key)?.let { decodeMediaRow(it, kind) }
        if (previous != null && !previous.degraded) {
            AppLog.i(TAG, "media $label: $fallbackFrom, $provider answered; kept the complete ${previous.media.sourceProvider} entry")
            return previous.media.withAltTitles(value.searchTitles())
        }
        val synonyms = cache.getStale(refKey("details", ref))
            ?.let { raw -> runCatching { json.decodeFromString(MediaDetails.serializer(), raw) }.getOrNull() }
            ?.synonyms.orEmpty()
        val merged = value.withAltTitles(previous?.media?.searchTitles().orEmpty() + synonyms)
        cache.put(key, encodeMediaRow(CachedMedia(merged, degraded = true)))
        AppLog.i(TAG, "media $label: $fallbackFrom, $provider answered; cached ${FALLBACK_TTL_MS / 60_000} min (degraded)")
        return merged
    }

    override suspend fun episodes(ref: MediaRef): List<Episode> {
        val key = "eps:${ref.anilistId}:${ref.malId}:${ref.kitsuId}"
        val ser = ListSerializer(Episode.serializer())
        cache.getFresh(key, detailTtlMs())?.let { raw ->
            runCatching { json.decodeFromString(ser, raw) }.getOrNull()?.let { return it }
        }
        return try {
            router.episodes(ref).value.also { if (it.isNotEmpty()) cache.put(key, json.encodeToString(ser, it)) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            cache.getStale(key)?.let { raw -> runCatching { json.decodeFromString(ser, raw) }.getOrNull() }
                ?: throw e.toAppException()
        }
    }

    // --- parity wave A (docs/redesign/15 §3.1, worker o): Descubrir y ficha through the DISCOVER providers (AniList) ---

    /**
     * Server-side filtered search / browse. Pages are cached [FEED_TTL_MS] (stale copy when offline). Without an enabled DISCOVER
     * provider it degrades to the plain [search] (text) or the Popular feed (browse) so Search keeps working on Kitsu alone.
     */
    override suspend fun searchAdvanced(kind: MediaKind, query: MediaSearchQuery, page: Int): FeedPage {
        val key = "adv:$kind:${json.encodeToString(MediaSearchQuery.serializer(), query)}:$page"
        cache.getFresh(key, FEED_TTL_MS)?.let { raw -> decodePage(raw)?.let { return it } }
        return try {
            router.discover(kind, { it.items.isEmpty() }) { it.searchAdvanced(kind, query, page) }.value
                .also { if (it.items.isNotEmpty()) cache.put(key, encodePage(it)) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: NoMetadataProviderException) {
            if (e.errors.isNotEmpty()) cache.getStale(key)?.let { decodePage(it) }?.let { return it }
            if (e.errors.isNotEmpty()) throw e.toAppException()
            val text = query.query?.trim().orEmpty()
            if (text.isNotEmpty()) search(kind, text, page) else feed(kind, FeedKind.POPULAR, page)
        } catch (e: Exception) {
            cache.getStale(key)?.let { decodePage(it) } ?: throw e.toAppException()
        }
    }

    override suspend fun details(ref: MediaRef): MediaDetails? =
        discoverCached(refKey("details", ref), detailTtlMs(), MediaDetails.serializer(), empty = null) { it.details(ref) }

    override suspend fun relations(ref: MediaRef): List<MediaRelation> =
        discoverCached(refKey("relations", ref), detailTtlMs(), ListSerializer(MediaRelation.serializer()), emptyList()) { it.relations(ref) }
            ?: emptyList()

    override suspend fun seasons(ref: MediaRef): List<Media> =
        discoverCached(refKey("seasons", ref), detailTtlMs(), ListSerializer(Media.serializer()), emptyList()) { it.seasons(ref) }
            ?: emptyList()

    override suspend fun characters(ref: MediaRef, page: Int): CharactersPage =
        discoverCached(refKey("chars:$page", ref), detailTtlMs(), CharactersPage.serializer(), CharactersPage(emptyList(), page, false)) {
            it.characters(ref, page)
        } ?: CharactersPage(emptyList(), page, false)

    override suspend fun staff(ref: MediaRef): List<StaffEdge> =
        discoverCached(refKey("staff", ref), detailTtlMs(), ListSerializer(StaffEdge.serializer()), emptyList()) { it.staff(ref) }
            ?: emptyList()

    override suspend fun recommendations(ref: MediaRef): List<Recommendation> =
        discoverCached(refKey("recs", ref), detailTtlMs(), ListSerializer(Recommendation.serializer()), emptyList()) { it.recommendations(ref) }
            ?: emptyList()

    override suspend fun character(id: Int): Character? =
        discoverCached("character:$id", detailTtlMs(), Character.serializer(), empty = null) { it.character(id) }

    /** Not cached: typing changes the query on every keystroke. */
    override suspend fun searchCharacters(query: String, page: Int): List<CharacterRef> {
        if (query.isBlank()) return emptyList()
        return discoverCached(null, 0, ListSerializer(CharacterRef.serializer()), emptyList()) { it.searchCharacters(query, page) }
            ?: emptyList()
    }

    /** Genre / tag catalogues change rarely: cached [CATALOGUE_TTL_MS] (7 days, the [FeedCache.purge] horizon). */
    override suspend fun genres(kind: MediaKind): List<String> =
        discoverCached("genres:$kind", CATALOGUE_TTL_MS, ListSerializer(String.serializer()), emptyList()) { it.genres(kind) }
            ?: emptyList()

    override suspend fun tags(kind: MediaKind): List<MediaTag> =
        discoverCached("tags:$kind", CATALOGUE_TTL_MS, ListSerializer(MediaTag.serializer()), emptyList()) { it.tags(kind) }
            ?: emptyList()

    /** trace.moe; never cached (the bytes are the key). Quota / queue errors become [AppError.RateLimited]. */
    override suspend fun searchByImage(image: ByteArray, mimeType: String): List<ImageMatch> {
        if (!router.supportsImageSearch) return emptyList()
        return try {
            router.searchByImage(image, mimeType)
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            throw e.toAppException()
        }
    }

    private fun refKey(prefix: String, ref: MediaRef): String = "$prefix:${ref.anilistId}:${ref.malId}:${ref.kitsuId}"

    /**
     * Cache-first DISCOVER call. A fresh copy wins; on failure a stale copy is served; with no DISCOVER provider enabled the [empty]
     * value is returned ("not available", the screen hides the section). Empty / null results are not cached. [key] null = no cache.
     */
    private suspend fun <T : Any> discoverCached(
        key: String?,
        ttlMs: Long,
        serializer: KSerializer<T>,
        empty: T?,
        call: suspend (DiscoverProvider) -> T?,
    ): T? {
        fun decode(raw: String): T? = runCatching { json.decodeFromString(serializer, raw) }.getOrNull()
        fun isEmpty(v: T?): Boolean = v == null || (v as? Collection<*>)?.isEmpty() == true || (v as? CharactersPage)?.items?.isEmpty() == true
        if (key != null) cache.getFresh(key, ttlMs)?.let { raw -> decode(raw)?.let { return it } }
        return try {
            router.discover(MediaKind.ANIME, ::isEmpty) { call(it) }.value
                .also { v -> if (key != null && v != null && !isEmpty(v)) cache.put(key, json.encodeToString(serializer, v)) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: NoMetadataProviderException) {
            if (e.errors.isEmpty()) return empty
            key?.let { cache.getStale(it) }?.let { decode(it) } ?: throw e.toAppException()
        } catch (e: Exception) {
            key?.let { cache.getStale(it) }?.let { decode(it) } ?: throw e.toAppException()
        }
    }

    // --- parity wave B (16 §3.1, worker r) ---

    @Serializable
    private data class CachedAiring(val items: List<CachedAiringRow>)

    @Serializable
    private data class CachedAiringRow(val media: Media, val episode: Int, val at: Long)

    /**
     * AniList `Page.airingSchedules` through the router (DISCOVER providers that implement `AiringProvider`), cached [FEED_TTL_MS] per
     * window and page (stale copy offline). No DISCOVER provider enabled = empty ("not available").
     */
    override suspend fun airingSchedule(fromEpochSec: Long, toEpochSec: Long, page: Int): List<AiringEpisode> {
        val key = "airing:$fromEpochSec:$toEpochSec:$page"
        fun decode(raw: String): List<AiringEpisode>? = runCatching { json.decodeFromString(CachedAiring.serializer(), raw) }.getOrNull()
            ?.items?.map { AiringEpisode(it.media, it.episode, it.at) }
        cache.getFresh(key, FEED_TTL_MS)?.let { raw -> decode(raw)?.let { return it } }
        return try {
            router.airingSchedule(fromEpochSec, toEpochSec, page).value.first.also { list ->
                if (list.isNotEmpty()) {
                    cache.put(key, json.encodeToString(CachedAiring.serializer(), CachedAiring(list.map { CachedAiringRow(it.media, it.episode, it.airingAtEpochSec) })))
                }
            }
        } catch (e: CancellationException) {
            throw e
        } catch (e: NoMetadataProviderException) {
            if (e.errors.isEmpty()) return emptyList()
            cache.getStale(key)?.let { decode(it) } ?: throw e.toAppException()
        } catch (e: Exception) {
            cache.getStale(key)?.let { decode(it) } ?: throw e.toAppException()
        }
    }

    /** Checklist settings.metadata.provider-status: the router's last result / cooldown per known provider, re-read every 2 s. */
    override fun providerStatus(): Flow<List<ProviderStatus>> = flow {
        while (true) {
            emit(router.statusSnapshot(AppSettings.KNOWN_PROVIDER_IDS))
            delay(STATUS_TICK_MS)
        }
    }.distinctUntilChanged()

    // --- android wave (12 §3.6) ---

    override suspend fun cacheStats(): MetadataCacheStats {
        val (entries, bytes) = cache.stats()
        return MetadataCacheStats(entries = entries, bytes = bytes)
    }

    override suspend fun clearCache() = cache.clear()

    private fun detailTtlMs(): Long = ttlHours().coerceIn(1, 168).toLong() * 60 * 60_000

    private fun encodePage(p: FeedPage): String =
        json.encodeToString(CachedPage.serializer(), CachedPage(p.items, p.page, p.hasNext))

    private fun decodePage(s: String): FeedPage? =
        runCatching { json.decodeFromString(CachedPage.serializer(), s) }.getOrNull()
            ?.let { FeedPage(it.items, it.page, it.hasNext) }

    private fun encodeMediaRow(row: CachedMedia): String = json.encodeToString(CachedMedia.serializer(), row)

    /**
     * A `media:` row. A bare [Media] (written before BUG-143) counts as degraded when it did not come from the primary provider
     * (e.g. the Kitsu row a 429 left on the phone in r12), so it is retried instead of served for a day.
     */
    private fun decodeMediaRow(s: String, kind: MediaKind): CachedMedia? {
        runCatching { json.decodeFromString(CachedMedia.serializer(), s) }.getOrNull()?.let { return it }
        val media = runCatching { json.decodeFromString(Media.serializer(), s) }.getOrNull() ?: return null
        val primary = router.primaryId(kind)
        return CachedMedia(media, degraded = primary != null && media.sourceProvider != primary)
    }

    private fun Exception.toAppException(): AppException = when (this) {
        is AppException -> this
        is NoMetadataProviderException -> AppException(
            rateLimitedProvider?.let { AppError.RateLimited(it) }
                ?: if (allOffline) AppError.Offline else AppError.Unknown(message.orEmpty()),
            this,
        )
        is ProviderException -> AppException(
            when {
                status == 0 -> AppError.Offline
                status == 402 || status == 429 -> AppError.RateLimited(provider)
                else -> AppError.Http(status)
            },
            this,
        )
        else -> AppException(AppError.Unknown(message ?: this::class.simpleName.orEmpty()), this)
    }

    private companion object {
        const val FEED_TTL_MS = 15L * 60_000
        /** BUG-143: a fallback `media:` row (the primary did not answer) is fresh this long, like the feeds. */
        const val FALLBACK_TTL_MS = 15L * 60_000
        const val TAG = "metadata"
        const val DEFAULT_TTL_HOURS = 24
        const val CATALOGUE_TTL_MS = 7L * 24 * 60 * 60_000
        const val STATUS_TICK_MS = 2_000L
    }
}
