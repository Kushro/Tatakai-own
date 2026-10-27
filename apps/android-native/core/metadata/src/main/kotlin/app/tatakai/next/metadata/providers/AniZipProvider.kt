package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.Mappers
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.metadata.fetchJson
import app.tatakai.next.model.Episode
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request

/** ani.zip: numbered episode lists (title, thumbnail, air date) and id mappings. Anime only, not a catalog. */
class AniZipProvider(
    private val client: OkHttpClient,
    private val baseUrl: String = "https://api.ani.zip",
) : MetadataProvider {
    override val id = ID
    override val kinds = setOf(MediaKind.ANIME)
    override val capabilities = setOf(Capability.EPISODES)

    override suspend fun episodes(ref: MediaRef): List<Episode> {
        val url = baseUrl.toHttpUrl().newBuilder().addPathSegment("mappings").apply {
            when {
                ref.anilistId != null -> addQueryParameter("anilist_id", ref.anilistId.toString())
                ref.malId != null -> addQueryParameter("mal_id", ref.malId.toString())
                ref.kitsuId != null -> addQueryParameter("kitsu_id", ref.kitsuId.toString())
                else -> return emptyList()
            }
        }.build()
        val raw = try {
            client.fetchJson(ID, Request.Builder().url(url).build()).obj()
        } catch (e: ProviderException) {
            if (e.status == 404) return emptyList() else throw e
        } ?: return emptyList()
        return Mappers.aniZipEpisodes(raw)
    }

    /** Full id mapping for [ref] (used by callers that need an AniList id from a MAL/Kitsu id). */
    suspend fun ids(ref: MediaRef): MediaRef? {
        val key = when {
            ref.anilistId != null -> "anilist_id" to ref.anilistId
            ref.malId != null -> "mal_id" to ref.malId
            ref.kitsuId != null -> "kitsu_id" to ref.kitsuId
            else -> return null
        }
        val url = baseUrl.toHttpUrl().newBuilder().addPathSegment("mappings")
            .addQueryParameter(key.first, key.second.toString()).build()
        val raw = try {
            client.fetchJson(ID, Request.Builder().url(url).build()).obj()
        } catch (e: ProviderException) {
            if (e.status == 404) return null else throw e
        } ?: return null
        return Mappers.aniZipIds(raw).takeIf { it.anilistId != null }
    }

    companion object {
        const val ID = "anizip"
    }
}
