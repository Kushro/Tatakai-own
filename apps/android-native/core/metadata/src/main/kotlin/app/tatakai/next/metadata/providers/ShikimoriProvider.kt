// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.metadata.providers

import app.tatakai.next.metadata.Capability
import app.tatakai.next.metadata.ExtraMappers
import app.tatakai.next.metadata.MetadataProvider
import app.tatakai.next.metadata.ProviderException
import app.tatakai.next.metadata.fetchJson
import app.tatakai.next.model.FeedPage
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody

/**
 * Parity wave B (16 §3.1, worker r, checklist metadata.providers.shikimori): Shikimori GraphQL (no key, ~5 req/s, mandatory
 * `User-Agent`). Port of the classic `providers/shikimori.ts`, which moved from `shikimori.one` to `shikimori.io` (the old domain
 * redirects). Shikimori ids ARE MAL ids, so `media` needs `MediaRef.malId` (passed by the router's gap filling from AniList's `idMal`):
 * alternative titles, score and synopsis (Russian-only texts are dropped).
 */
class ShikimoriProvider(
    private val client: OkHttpClient,
    private val endpoint: String = GRAPHQL_URL,
) : MetadataProvider {
    override val id: String = ID
    override val kinds: Set<MediaKind> = setOf(MediaKind.ANIME, MediaKind.MANGA)
    override val capabilities: Set<Capability> = setOf(Capability.SEARCH, Capability.MEDIA)

    private fun field(kind: MediaKind) = if (kind == MediaKind.MANGA) "mangas" else "animes"

    private fun card(kind: MediaKind) = if (kind == MediaKind.MANGA) MANGA_CARD else ANIME_CARD

    override suspend fun search(kind: MediaKind, query: String, page: Int): FeedPage {
        val q = query.trim()
        val p = page.coerceAtLeast(1)
        if (q.isEmpty()) return FeedPage(emptyList(), p, false)
        val doc = post(
            "query(\$page: Int, \$limit: Int, \$search: String) { ${field(kind)}(page: \$page, limit: \$limit, search: \$search, censored: true) { ${card(kind)} } }",
            buildJsonObject {
                put("page", p)
                put("limit", PER_PAGE)
                put("search", q)
            },
        )
        val items = ExtraMappers.shikiList(doc, kind)
        return FeedPage(items, p, items.size >= PER_PAGE)
    }

    override suspend fun media(kind: MediaKind, ref: MediaRef): Media? {
        val malId = ref.malId ?: return null
        val doc = post(
            "query(\$ids: String) { ${field(kind)}(ids: \$ids, limit: 1) { ${card(kind)} description } }",
            buildJsonObject { put("ids", malId.toString()) },
        )
        val m = ExtraMappers.shikiList(doc, kind).firstOrNull() ?: return null
        return m.copy(ref = m.ref.copy(anilistId = ref.anilistId, kitsuId = ref.kitsuId))
    }

    private suspend fun post(query: String, variables: JsonObject): JsonElement {
        val body = buildJsonObject {
            put("query", query)
            put("variables", variables)
        }.toString().toRequestBody(JSON)
        val req = Request.Builder().url(endpoint).post(body)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .build()
        val doc = client.fetchJson(ID, req)
        val errors = (doc as? JsonObject)?.get("errors") as? JsonArray
        val data = (doc as? JsonObject)?.get("data")
        if (!errors.isNullOrEmpty() && (data == null || data is JsonNull)) throw ProviderException(ID, 502, "GraphQL error")
        return doc
    }

    companion object {
        const val ID: String = "shikimori"
        const val GRAPHQL_URL: String = "https://shikimori.io/api/graphql"
        private const val USER_AGENT = "Tatakai-own (android)"
        private const val PER_PAGE = 20
        private val JSON = "application/json".toMediaType()
        private const val CARD = "id malId name english japanese synonyms kind status score poster { originalUrl mainUrl } airedOn { year month day } genres { id name kind }"
        private const val ANIME_CARD = "$CARD episodes season"
        private const val MANGA_CARD = "$CARD chapters volumes"
    }
}
