// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.FormBody
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response

// AniList (GraphQL) and MyAnimeList (REST v2) clients of the lists layer (worker q). Hosts: graphql.anilist.co, anilist.co/api/v2/oauth,
// api.myanimelist.net, myanimelist.net/v1/oauth2 only. Tokens travel only to their own provider; nothing is logged.

/** A favorite anime or character as AniList lists it. */
data class FavoriteNode(val id: Int, val name: String, val image: String?, val malId: Int? = null, val episodes: Int? = null)

/** The account behind a token. [scoreFormat]: AniList `mediaListOptions.scoreFormat`, or "mal". */
data class RemoteAccount(val userId: String, val username: String, val avatar: String?, val scoreFormat: String?)

/** One provider as the sync sees it. Every call throws [AppException] (`Http(401)` = token rejected). */
interface ListRemote {
    val provider: ListProvider

    suspend fun viewer(token: String): RemoteAccount

    /** The whole anime list. */
    suspend fun fetchAll(token: String, account: RemoteAccount): List<ListEntry>

    /** The current remote values of one title; null = not on the list. [id] = the provider's id. */
    suspend fun fetchOne(token: String, id: Int): ListEntry?

    suspend fun save(token: String, id: Int, entry: ListEntry)

    suspend fun delete(token: String, id: Int)

    /** Favorite anime ids (AniList only; MAL v2 has no favorites API). */
    suspend fun favoriteAnime(token: String): Set<Int> = emptySet()

    suspend fun favoriteCharacters(token: String): Set<Int> = emptySet()

    /** AniList `ToggleFavourite` (a toggle: callers check the current state first). */
    suspend fun toggleFavorite(token: String, animeId: Int? = null, characterId: Int? = null) {}

    /** The provider's id of a title from what is known locally. */
    fun idOf(entry: ListEntry): Int? = when (provider) {
        ListProvider.ANILIST -> entry.ref.anilistId
        ListProvider.MAL -> entry.ref.malId
    }
}

private val JSON_TYPE = "application/json; charset=utf-8".toMediaType()

internal suspend fun OkHttpClient.callText(request: Request, provider: String): String = withContext(Dispatchers.IO) {
    val response: Response = try {
        newCall(request).execute()
    } catch (e: java.io.IOException) {
        throw AppException(AppError.Offline, e)
    }
    response.use { r ->
        val text = r.body.string()
        when {
            r.code == 429 -> throw AppException(AppError.RateLimited(provider))
            !r.isSuccessful -> throw AppException(AppError.Http(r.code))
            else -> text
        }
    }
}

// ==================================================================== AniList

class AniListListClient(private val client: OkHttpClient, private val endpoint: String = "https://graphql.anilist.co") : ListRemote {
    override val provider = ListProvider.ANILIST

    /** POST a GraphQL document; returns `data`. GraphQL errors with a 401-like message map to Http(401). */
    suspend fun gql(query: String, variables: JsonObject, token: String?): JsonObject {
        val body = buildJsonObject {
            put("query", query)
            put("variables", variables)
        }.toString().toRequestBody(JSON_TYPE)
        val request = Request.Builder().url(endpoint).post(body)
            .header("Accept", "application/json")
            .apply { if (token != null) header("Authorization", "Bearer $token") }
            .build()
        // AniList answers an invalid token with HTTP 400/401 and `errors[].status = 401`; both surface as AppError.Http.
        val text = client.callText(request, "AniList")
        val root = jsonObjectOf(text)
        val errors = root.arr("errors")?.objectList().orEmpty()
        if (errors.isNotEmpty() && root.obj("data") == null) {
            val status = errors.firstNotNullOfOrNull { it.int("status") } ?: 400
            throw AppException(AppError.Http(status))
        }
        return root.obj("data") ?: throw AppException(AppError.Http(502))
    }

    override suspend fun viewer(token: String): RemoteAccount {
        val data = gql(
            "query { Viewer { id name avatar { large medium } mediaListOptions { scoreFormat } } }",
            JsonObject(emptyMap()),
            token,
        )
        val v = data.obj("Viewer") ?: throw AppException(AppError.Http(401))
        return RemoteAccount(
            userId = v.int("id")?.toString() ?: throw AppException(AppError.Http(401)),
            username = v.str("name").orEmpty(),
            avatar = v.obj("avatar")?.let { it.str("large") ?: it.str("medium") },
            scoreFormat = v.obj("mediaListOptions")?.str("scoreFormat"),
        )
    }

    override suspend fun fetchAll(token: String, account: RemoteAccount): List<ListEntry> {
        val out = LinkedHashMap<Int, ListEntry>()
        for (chunk in 1..MAX_CHUNKS) {
            val data = gql(COLLECTION_QUERY, vars("userId" to account.userId.toInt(), "chunk" to chunk), token)
            val (entries, more) = parseAniListCollection(data)
            entries.forEach { e -> e.ref.anilistId?.let { out.putIfAbsent(it, e) } }
            if (!more) break
        }
        return out.values.toList()
    }

    override suspend fun fetchOne(token: String, id: Int): ListEntry? {
        val data = gql(
            "query (\$id: Int) { Media(id: \$id, type: ANIME) { id idMal episodes title { userPreferred romaji } coverImage { large } " +
                "isFavourite mediaListEntry { $ENTRY_FIELDS } } }",
            vars("id" to id),
            token,
        )
        val media = data.obj("Media") ?: return null
        val entry = media.obj("mediaListEntry") ?: return null
        return parseAniListEntry(JsonObject(entry + ("media" to media)))
    }

    override suspend fun save(token: String, id: Int, entry: ListEntry) {
        gql(
            "mutation (\$mediaId: Int, \$status: MediaListStatus, \$progress: Int, \$scoreRaw: Int) { " +
                "SaveMediaListEntry(mediaId: \$mediaId, status: \$status, progress: \$progress, scoreRaw: \$scoreRaw) { id } }",
            buildJsonObject {
                put("mediaId", id)
                put("status", entry.status.toAniList())
                put("progress", entry.progress.coerceAtLeast(0))
                put("scoreRaw", (entry.score ?: 0).coerceIn(0, 100))
            },
            token,
        )
    }

    override suspend fun delete(token: String, id: Int) {
        val data = gql("query (\$id: Int) { Media(id: \$id) { mediaListEntry { id } } }", vars("id" to id), token)
        val entryId = data.obj("Media")?.obj("mediaListEntry")?.int("id") ?: return // not on the list: nothing to do
        gql("mutation (\$id: Int) { DeleteMediaListEntry(id: \$id) { deleted } }", vars("id" to entryId), token)
    }

    override suspend fun favoriteAnime(token: String): Set<Int> = favourites(token, "anime", "id").mapNotNull { it.int("id") }.toSet()

    override suspend fun favoriteCharacters(token: String): Set<Int> = favoriteCharacterNodes(token).map { it.id }.toSet()

    /** Favorite anime with what the Favorites tab shows (titles that are on no list still appear there). */
    suspend fun favoriteAnimeNodes(token: String): List<FavoriteNode> =
        favourites(token, "anime", "id idMal episodes title { userPreferred romaji } coverImage { large medium }").mapNotNull { n ->
            val id = n.int("id") ?: return@mapNotNull null
            FavoriteNode(
                id = id,
                malId = n.int("idMal"),
                name = n.obj("title")?.let { it.str("userPreferred") ?: it.str("romaji") } ?: "#$id",
                image = n.obj("coverImage")?.let { it.str("large") ?: it.str("medium") },
                episodes = n.int("episodes"),
            )
        }

    suspend fun favoriteCharacterNodes(token: String): List<FavoriteNode> =
        favourites(token, "characters", "id name { userPreferred full } image { large medium }").mapNotNull { n ->
            val id = n.int("id") ?: return@mapNotNull null
            FavoriteNode(
                id = id,
                name = n.obj("name")?.let { it.str("userPreferred") ?: it.str("full") } ?: "#$id",
                image = n.obj("image")?.let { it.str("large") ?: it.str("medium") },
            )
        }

    /** Whether the viewer has [animeId] as a favorite right now (before a toggle). */
    suspend fun isFavouriteAnime(token: String, animeId: Int): Boolean =
        gql("query (\$id: Int) { Media(id: \$id) { isFavourite } }", vars("id" to animeId), token).obj("Media")?.bool("isFavourite") == true

    suspend fun isFavouriteCharacter(token: String, characterId: Int): Boolean =
        gql("query (\$id: Int) { Character(id: \$id) { isFavourite } }", vars("id" to characterId), token).obj("Character")?.bool("isFavourite") == true

    private suspend fun favourites(token: String, field: String, nodeFields: String): List<JsonObject> {
        val out = ArrayList<JsonObject>()
        for (page in 1..MAX_FAVORITE_PAGES) {
            val data = gql(
                "query (\$page: Int) { Viewer { favourites { $field(page: \$page, perPage: 50) { pageInfo { hasNextPage } nodes { $nodeFields } } } } }",
                vars("page" to page),
                token,
            )
            val conn = data.obj("Viewer")?.obj("favourites")?.obj(field) ?: break
            conn.arr("nodes")?.objectList()?.let(out::addAll)
            if (conn.obj("pageInfo")?.bool("hasNextPage") != true) break
        }
        return out
    }

    override suspend fun toggleFavorite(token: String, animeId: Int?, characterId: Int?) {
        gql(
            "mutation (\$animeId: Int, \$characterId: Int) { ToggleFavourite(animeId: \$animeId, characterId: \$characterId) { anime { pageInfo { total } } } }",
            buildJsonObject {
                if (animeId != null) put("animeId", animeId)
                if (characterId != null) put("characterId", characterId)
            },
            token,
        )
    }

    /** Public lookup (no token): the AniList id of a MAL id, or the MAL id of an AniList id. */
    suspend fun crossId(anilistId: Int?, malId: Int?): Pair<Int?, Int?> {
        val data = if (anilistId != null) {
            gql("query (\$id: Int) { Media(id: \$id, type: ANIME) { id idMal } }", vars("id" to anilistId), null)
        } else {
            gql("query (\$idMal: Int) { Media(idMal: \$idMal, type: ANIME) { id idMal } }", vars("idMal" to (malId ?: return null to null)), null)
        }
        val m = data.obj("Media") ?: return anilistId to malId
        return (m.int("id") ?: anilistId) to (m.int("idMal") ?: malId)
    }

    /**
     * Token exchange of the authorization-code grant (`POST https://anilist.co/api/v2/oauth/token`). AniList requires the client secret
     * for this grant, which a public mobile client cannot ship, so `beginLink` uses the implicit grant; this path only serves a code
     * redirect (it fails with `oauth:exchange` unless AniList accepts the public client).
     */
    suspend fun exchangeCode(clientId: String, code: String, redirectUri: String): TokenSet {
        val body = buildJsonObject {
            put("grant_type", "authorization_code")
            put("client_id", clientId)
            put("redirect_uri", redirectUri)
            put("code", code)
        }.toString().toRequestBody(JSON_TYPE)
        val request = Request.Builder().url(TOKEN_URL).post(body).header("Accept", "application/json").build()
        val text = try {
            client.callText(request, "AniList")
        } catch (e: AppException) {
            throw AppException(AppError.Unknown(ListErrors.EXCHANGE), e)
        }
        return parseTokenAnswer(text, System.currentTimeMillis())
    }

    companion object {
        const val AUTHORIZE_URL = "https://anilist.co/api/v2/oauth/authorize"
        const val TOKEN_URL = "https://anilist.co/api/v2/oauth/token"
        private const val MAX_CHUNKS = 40
        private const val MAX_FAVORITE_PAGES = 40
        private const val ENTRY_FIELDS =
            "id mediaId status progress score(format: POINT_100) updatedAt notes startedAt { year month day } completedAt { year month day }"
        private const val COLLECTION_QUERY =
            "query (\$userId: Int, \$chunk: Int) { MediaListCollection(userId: \$userId, type: ANIME, chunk: \$chunk, perChunk: 500, " +
                "forceSingleCompletedList: true) { hasNextChunk lists { entries { $ENTRY_FIELDS media { id idMal episodes isFavourite " +
                "title { userPreferred romaji english native } coverImage { large medium } } } } } }"

        /**
         * Implicit grant URL (the token comes back in the redirect fragment). No `redirect_uri`: AniList's implicit grant
         * always redirects to the URL registered on the client (`tatakai://oauth/anilist`) and refuses the request with
         * `unsupported_grant_type` when the parameter is present (same URL as the desktop's `anilist_authorize_url`).
         */
        fun authorizeUrl(clientId: String, state: String): String =
            "$AUTHORIZE_URL?client_id=${OAuthRedirect.encode(clientId)}" +
                "&response_type=token&state=${OAuthRedirect.encode(state)}"

        private fun vars(vararg pairs: Pair<String, Any>): JsonObject = JsonObject(
            pairs.associate { (k, v) ->
                k to when (v) {
                    is Int -> JsonPrimitive(v)
                    is String -> JsonPrimitive(v)
                    is Boolean -> JsonPrimitive(v)
                    else -> JsonNull as JsonElement
                }
            },
        )
    }
}

// ==================================================================== MyAnimeList

class MalListClient(
    private val client: OkHttpClient,
    private val api: String = "https://api.myanimelist.net/v2",
    private val tokenUrl: String = TOKEN_URL,
) : ListRemote {
    override val provider = ListProvider.MAL

    private fun authed(url: String, token: String) = Request.Builder().url(url).header("Authorization", "Bearer $token").header("Accept", "application/json")

    override suspend fun viewer(token: String): RemoteAccount {
        val o = jsonObjectOf(client.callText(authed("$api/users/@me", token).get().build(), "MyAnimeList"))
        return RemoteAccount(
            userId = o.int("id")?.toString() ?: throw AppException(AppError.Http(401)),
            username = o.str("name").orEmpty(),
            avatar = o.str("picture"),
            scoreFormat = "mal",
        )
    }

    override suspend fun fetchAll(token: String, account: RemoteAccount): List<ListEntry> {
        val out = ArrayList<ListEntry>()
        var url: String? = "$api/users/@me/animelist?fields=list_status,num_episodes&limit=1000&nsfw=true"
        var pages = 0
        while (url != null && pages < MAX_PAGES) {
            val (items, next) = parseMalPage(jsonObjectOf(client.callText(authed(url, token).get().build(), "MyAnimeList")))
            out += items
            url = next
            pages++
        }
        return out
    }

    override suspend fun fetchOne(token: String, id: Int): ListEntry? {
        val o = jsonObjectOf(client.callText(authed("$api/anime/$id?fields=my_list_status,num_episodes", token).get().build(), "MyAnimeList"))
        if (o.obj("my_list_status") == null) return null
        return parseMalItem(JsonObject(mapOf("node" to o)))
    }

    override suspend fun save(token: String, id: Int, entry: ListEntry) {
        val (status, rewatching) = entry.status.toMal()
        val form = FormBody.Builder()
            .add("status", status)
            .add("is_rewatching", rewatching.toString())
            .add("num_watched_episodes", entry.progress.coerceAtLeast(0).toString())
            .add("score", toMalScore(entry.score).toString())
            .build()
        client.callText(authed("$api/anime/$id/my_list_status", token).patch(form).build(), "MyAnimeList")
    }

    override suspend fun delete(token: String, id: Int) {
        try {
            client.callText(authed("$api/anime/$id/my_list_status", token).delete().build(), "MyAnimeList")
        } catch (e: AppException) {
            if ((e.error as? AppError.Http)?.code != 404) throw e // 404 = not on the list
        }
    }

    /** `POST https://myanimelist.net/v1/oauth2/token` (authorization code + PKCE plain). */
    suspend fun exchangeCode(clientId: String, code: String, verifier: String, redirectUri: String): TokenSet =
        tokenCall(
            FormBody.Builder()
                .add("client_id", clientId)
                .add("grant_type", "authorization_code")
                .add("code", code)
                .add("redirect_uri", redirectUri)
                .add("code_verifier", verifier)
                .build(),
        )

    suspend fun refresh(clientId: String, refreshToken: String): TokenSet =
        tokenCall(FormBody.Builder().add("client_id", clientId).add("grant_type", "refresh_token").add("refresh_token", refreshToken).build())

    private suspend fun tokenCall(form: FormBody): TokenSet {
        val text = try {
            client.callText(Request.Builder().url(tokenUrl).post(form).header("Accept", "application/json").build(), "MyAnimeList")
        } catch (e: AppException) {
            val code = (e.error as? AppError.Http)?.code
            throw AppException(AppError.Unknown(if (code == 400 || code == 401) ListErrors.EXPIRED else ListErrors.EXCHANGE), e)
        }
        return parseTokenAnswer(text, System.currentTimeMillis())
    }

    companion object {
        const val AUTHORIZE_URL = "https://myanimelist.net/v1/oauth2/authorize"
        const val TOKEN_URL = "https://myanimelist.net/v1/oauth2/token"
        private const val MAX_PAGES = 50

        fun authorizeUrl(clientId: String, state: String, verifier: String): String =
            "$AUTHORIZE_URL?response_type=code&client_id=${OAuthRedirect.encode(clientId)}" +
                "&redirect_uri=${OAuthRedirect.encode(OAuthRedirect.redirectUri(ListProvider.MAL))}" +
                "&state=${OAuthRedirect.encode(state)}" +
                "&code_challenge=${OAuthRedirect.encode(Pkce.plainChallenge(verifier))}&code_challenge_method=plain"
    }
}

/** `{ access_token, refresh_token?, expires_in? }` -> [TokenSet]. */
fun parseTokenAnswer(text: String, nowMs: Long): TokenSet {
    val o = jsonObjectOf(text)
    val access = o.str("access_token")?.takeIf { it.isNotBlank() } ?: throw AppException(AppError.Unknown(ListErrors.EXCHANGE))
    val expiresIn = o.long("expires_in")
    return TokenSet(access = access, refresh = o.str("refresh_token")?.takeIf { it.isNotBlank() }, expiresAtMs = expiresIn?.let { nowMs + it * 1000 })
}
