package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.ArtworkProvider
import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.Mappers
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.metadata.fetchJson
import app.tatakai.next.model.Episode
import app.tatakai.next.model.MediaArtwork
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.serialization.json.JsonObject
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request

/**
 * ani.zip: numbered episode lists (title, thumbnail, air date, synopsis, length, rating), id mappings and (0.3.2) TVDB artwork.
 * Anime only, not a catalog. One `/mappings` answer serves the episodes, the artwork and the ids of a title: it is kept in memory for
 * [memoTtlMs] so Details asking for episodes and artwork costs one request (the repository caches both answers in Room).
 */
class AniZipProvider(
    private val client: OkHttpClient,
    private val baseUrl: String = "https://api.ani.zip",
    private val clock: () -> Long = System::currentTimeMillis,
    private val memoTtlMs: Long = 10 * 60_000L,
) : MetadataProvider, ArtworkProvider {
    override val id = ID
    override val kinds = setOf(MediaKind.ANIME)
    override val capabilities = setOf(Capability.EPISODES)

    private class Memo(val at: Long, val raw: JsonObject?)

    private val memo = object : LinkedHashMap<String, Memo>(16, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, Memo>?): Boolean = size > MEMO_SIZE
    }

    override suspend fun episodes(ref: MediaRef): List<Episode> = mappings(ref)?.let(Mappers::aniZipEpisodes).orEmpty()

    override suspend fun artwork(ref: MediaRef): MediaArtwork? = mappings(ref)?.let(Mappers::aniZipArtwork)

    /** Full id mapping for [ref] (used by callers that need an AniList id from a MAL/Kitsu id). */
    suspend fun ids(ref: MediaRef): MediaRef? = mappings(ref)?.let(Mappers::aniZipIds)?.takeIf { it.anilistId != null }

    /** The `/mappings` object of [ref] (AniList id first); null when ani.zip does not know it (404) or [ref] has no id. */
    private suspend fun mappings(ref: MediaRef): JsonObject? {
        val key = when {
            ref.anilistId != null -> "anilist_id" to ref.anilistId
            ref.malId != null -> "mal_id" to ref.malId
            ref.kitsuId != null -> "kitsu_id" to ref.kitsuId
            else -> return null
        }
        val memoKey = "${key.first}=${key.second}"
        val now = clock()
        synchronized(memo) { memo[memoKey]?.takeIf { now - it.at <= memoTtlMs }?.let { return it.raw } }
        val url = baseUrl.toHttpUrl().newBuilder().addPathSegment("mappings")
            .addQueryParameter(key.first, key.second.toString()).build()
        val raw = try {
            client.fetchJson(ID, Request.Builder().url(url).build()).obj()
        } catch (e: ProviderException) {
            if (e.status == 404) null else throw e
        }
        synchronized(memo) { memo[memoKey] = Memo(now, raw) }
        return raw
    }

    companion object {
        const val ID = "anizip"
        private const val MEMO_SIZE = 32
    }
}
