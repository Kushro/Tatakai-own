package app.tatakai.next.data

import app.tatakai.next.database.FeedCache
import app.tatakai.next.metadata.MetadataRouter
import app.tatakai.next.metadata.NoMetadataProviderException
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import kotlinx.coroutines.CancellationException
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json

/**
 * Metadata through [MetadataRouter] with a Room-backed response cache (TTL). When every provider fails, a stale
 * cached copy is served instead of an error; with nothing cached an [AppException] carrying an [AppError] is thrown.
 */
class MetadataRepositoryImpl(
    private val router: MetadataRouter,
    private val cache: FeedCache,
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

    override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
        val key = "media:$kind:${ref.anilistId}:${ref.malId}:${ref.kitsuId}"
        cache.getFresh(key, MEDIA_TTL_MS)?.let { raw -> decodeMedia(raw)?.let { return it } }
        return try {
            router.media(kind, ref).value?.also { cache.put(key, json.encodeToString(Media.serializer(), it)) }
        } catch (e: CancellationException) {
            throw e
        } catch (e: Exception) {
            cache.getStale(key)?.let { decodeMedia(it) } ?: throw e.toAppException()
        }
    }

    override suspend fun episodes(ref: MediaRef): List<Episode> {
        val key = "eps:${ref.anilistId}:${ref.malId}:${ref.kitsuId}"
        val ser = ListSerializer(Episode.serializer())
        cache.getFresh(key, EPISODES_TTL_MS)?.let { raw ->
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

    private fun encodePage(p: FeedPage): String =
        json.encodeToString(CachedPage.serializer(), CachedPage(p.items, p.page, p.hasNext))

    private fun decodePage(s: String): FeedPage? =
        runCatching { json.decodeFromString(CachedPage.serializer(), s) }.getOrNull()
            ?.let { FeedPage(it.items, it.page, it.hasNext) }

    private fun decodeMedia(s: String): Media? =
        runCatching { json.decodeFromString(Media.serializer(), s) }.getOrNull()

    private fun Exception.toAppException(): AppException = when (this) {
        is AppException -> this
        is NoMetadataProviderException -> AppException(
            rateLimitedProvider?.let { AppError.RateLimited(it) }
                ?: if (allOffline) AppError.Offline else AppError.Unknown(message.orEmpty()),
            this,
        )
        else -> AppException(AppError.Unknown(message ?: this::class.simpleName.orEmpty()), this)
    }

    private companion object {
        const val FEED_TTL_MS = 15L * 60_000
        const val MEDIA_TTL_MS = 6L * 60 * 60_000
        const val EPISODES_TTL_MS = 6L * 60 * 60_000
    }
}
