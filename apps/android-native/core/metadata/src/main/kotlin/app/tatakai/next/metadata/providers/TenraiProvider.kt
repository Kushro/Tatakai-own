// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.ExtraMappers
import app.tatakai.next.metadata.Mappers.obj
import app.tatakai.next.metadata.Mappers.sub
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.metadata.fetchJson
import app.tatakai.next.model.Episode
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import okhttp3.Request

/**
 * Parity wave B (16 §3.1, worker r, checklist metadata.providers.tenrai): Tenrai (`api.tenrai.org/v1`, Jikan v4 schema over MAL ids;
 * public, no key, 120 req/min and 4 req/s: a `429` trips the router's cooldown). Port of the classic `providers/tenrai.ts` +
 * `lib/tenrai/client.ts`: feed lanes (`/top/anime`, `/seasons/now`), search (`/anime?q=`), details (`/anime/{id}/full`) and the episode
 * list (`/anime/{id}/episodes?page=`) for titles AniList / Kitsu / ani.zip do not know. Never sends `X-Server-Key`.
 */
class TenraiProvider(
    private val client: OkHttpClient,
    private val baseUrl: String = BASE_URL,
) : MetadataProvider {
    override val id: String = ID
    override val kinds: Set<MediaKind> = setOf(MediaKind.ANIME, MediaKind.MANGA)
    override val capabilities: Set<Capability> = setOf(Capability.FEED, Capability.SEARCH, Capability.MEDIA, Capability.EPISODES)

    private fun path(kind: MediaKind) = if (kind == MediaKind.MANGA) "manga" else "anime"

    private suspend fun get(url: okhttp3.HttpUrl) = client.fetchJson(ID, Request.Builder().url(url).header("Accept", "application/json").build())

    override suspend fun feed(kind: MediaKind, feed: FeedKind, page: Int): FeedPage {
        val p = page.coerceAtLeast(1)
        val b = baseUrl.toHttpUrl().newBuilder()
        when {
            feed == FeedKind.SEASONAL && kind == MediaKind.ANIME -> b.addPathSegment("seasons").addPathSegment("now")
            feed == FeedKind.RECENTLY_COMPLETED -> b.addPathSegment(path(kind))
                .addQueryParameter("status", "complete").addQueryParameter("order_by", "end_date").addQueryParameter("sort", "desc")
            else -> {
                b.addPathSegment("top").addPathSegment(path(kind))
                val filter = when (feed) {
                    FeedKind.TRENDING, FeedKind.AIRING -> if (kind == MediaKind.MANGA) "publishing" else "airing"
                    FeedKind.POPULAR -> "bypopularity"
                    FeedKind.UPCOMING -> "upcoming"
                    else -> null
                }
                filter?.let { b.addQueryParameter("filter", it) }
            }
        }
        b.addQueryParameter("page", p.toString()).addQueryParameter("limit", PER_PAGE.toString()).addQueryParameter("sfw", "true")
        return ExtraMappers.jikanPage(get(b.build()), kind, p)
    }

    override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage {
        val q = query.trim()
        val p = page.coerceAtLeast(1)
        if (q.isEmpty()) return FeedPage(emptyList(), p, false)
        val url = baseUrl.toHttpUrl().newBuilder().addPathSegment(path(kind))
            .addQueryParameter("q", q).addQueryParameter("page", p.toString())
            .addQueryParameter("limit", PER_PAGE.toString()).addQueryParameter("sfw", "true").build()
        return ExtraMappers.jikanPage(get(url), kind, p)
    }

    override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
        val malId = ref.malId ?: return null
        val url = baseUrl.toHttpUrl().newBuilder().addPathSegment(path(kind)).addPathSegment(malId.toString()).addPathSegment("full").build()
        val doc = try {
            get(url)
        } catch (e: ProviderException) {
            if (e.status == 404) return null else throw e
        }
        val m = doc.obj()?.sub("data")?.let { ExtraMappers.jikanEntry(it, kind) } ?: return null
        return m.copy(ref = m.ref.copy(anilistId = ref.anilistId, kitsuId = ref.kitsuId))
    }

    override suspend fun episodes(ref: MediaRef): List<Episode> {
        val malId = ref.malId ?: return emptyList()
        val out = mutableListOf<Episode>()
        var page = 1
        while (page <= EPISODE_PAGE_CAP) {
            val url = baseUrl.toHttpUrl().newBuilder().addPathSegment("anime").addPathSegment(malId.toString())
                .addPathSegment("episodes").addQueryParameter("page", page.toString()).build()
            val doc = try {
                get(url)
            } catch (e: ProviderException) {
                if (e.status == 404) break else throw e
            }
            val (list, hasNext) = ExtraMappers.jikanEpisodes(doc)
            out += list
            if (!hasNext || list.isEmpty()) break
            page++
        }
        return out.distinctBy { it.number }.sortedBy { it.number }
    }

    companion object {
        const val ID: String = "tenrai"
        const val BASE_URL: String = "https://api.tenrai.org/v1"
        private const val PER_PAGE = 24
        private const val EPISODE_PAGE_CAP = 40
    }
}
