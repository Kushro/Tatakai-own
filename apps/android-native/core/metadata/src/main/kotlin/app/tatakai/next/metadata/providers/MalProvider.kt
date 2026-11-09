// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.ExtraMappers
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.metadata.fetchJson
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r, checklist metadata.providers.mal): MyAnimeList
 * (`api.myanimelist.net/v2`, `X-MAL-CLIENT-ID` = the same `TATAKAI_MAL_CLIENT_ID` the lists use; without it the provider reports
 * `ProviderException(status = 401)` and the router skips it and cools it down). Port of the classic `providers/mal.ts`: feed lanes over
 * `/anime/ranking`, text search, and details by `MediaRef.malId` (the router's gap filling passes the `idMal` AniList returned, so a
 * title without AniList score / synopsis gets MAL's).
 */
class MalProvider(
    private val client: OkHttpClient,
    /** Koin property PROP_MAL_CLIENT_ID (same id as the lists OAuth); null / blank = provider unavailable. */
    private val clientId: () -> String? = { null },
    private val baseUrl: String = BASE_URL,
) : MetadataProvider {
    override val id: String = ID
    override val kinds: Set<MediaKind> = setOf(MediaKind.ANIME, MediaKind.MANGA)
    override val capabilities: Set<Capability> = setOf(Capability.FEED, Capability.SEARCH, Capability.MEDIA)

    private fun path(kind: MediaKind) = if (kind == MediaKind.MANGA) "manga" else "anime"

    private fun fields(kind: MediaKind) = if (kind == MediaKind.MANGA) MANGA_FIELDS else ANIME_FIELDS

    private suspend fun get(url: HttpUrl) = run {
        val key = clientId()?.trim()?.takeIf { it.isNotEmpty() } ?: throw ProviderException(ID, 401, "no MAL client id")
        client.fetchJson(ID, Request.Builder().url(url).header("X-MAL-CLIENT-ID", key).header("Accept", "application/json").build())
    }

    override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
        val p = page.coerceAtLeast(1)
        val rankingType = when (feed) {
            FeedKind.TRENDING, FeedKind.AIRING -> if (kind == MediaKind.MANGA) "manga" else "airing"
            FeedKind.POPULAR -> "bypopularity"
            FeedKind.TOP_RATED -> "all"
            FeedKind.UPCOMING -> if (kind == MediaKind.MANGA) "manga" else "upcoming"
            FeedKind.SEASONAL, FeedKind.RECENTLY_COMPLETED -> if (kind == MediaKind.MANGA) "manga" else "tv"
        }
        val url = baseUrl.toHttpUrl().newBuilder().addPathSegment(path(kind)).addPathSegment("ranking")
            .addQueryParameter("ranking_type", rankingType)
            .addQueryParameter("limit", PER_PAGE.toString())
            .addQueryParameter("offset", ((p - 1) * PER_PAGE).toString())
            .addQueryParameter("fields", fields(kind))
            .build()
        return ExtraMappers.malPage(get(url), kind, p)
    }

    override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage {
        val q = query.trim()
        val p = page.coerceAtLeast(1)
        // MAL rejects queries shorter than 3 characters with 400.
        if (q.length < 3) return FeedPage(emptyList(), p, false)
        val url = baseUrl.toHttpUrl().newBuilder().addPathSegment(path(kind))
            .addQueryParameter("q", q.take(64))
            .addQueryParameter("limit", PER_PAGE.toString())
            .addQueryParameter("offset", ((p - 1) * PER_PAGE).toString())
            .addQueryParameter("fields", fields(kind))
            .build()
        return ExtraMappers.malPage(get(url), kind, p)
    }

    override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
        val malId = ref.malId ?: return null
        val url = baseUrl.toHttpUrl().newBuilder().addPathSegment(path(kind)).addPathSegment(malId.toString())
            .addQueryParameter("fields", fields(kind)).build()
        val doc = try {
            get(url)
        } catch (e: ProviderException) {
            if (e.status == 404) return null else throw e
        }
        val m = doc.obj()?.let { ExtraMappers.malNode(it, kind) } ?: return null
        return m.copy(ref = m.ref.copy(anilistId = ref.anilistId, kitsuId = ref.kitsuId))
    }

    companion object {
        const val ID: String = "mal"
        const val BASE_URL: String = "https://api.myanimelist.net/v2"
        private const val PER_PAGE = 20
        private const val ANIME_FIELDS =
            "id,title,main_picture,alternative_titles,start_date,synopsis,mean,rank,genres,media_type,status,num_episodes,start_season"
        private const val MANGA_FIELDS = "id,title,main_picture,alternative_titles,start_date,synopsis,mean,rank,genres,media_type,status,num_chapters"
    }
}
