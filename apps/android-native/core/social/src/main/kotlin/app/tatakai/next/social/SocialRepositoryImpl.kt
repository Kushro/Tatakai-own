// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import app.tatakai.next.model.ActivityReply
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.Review
import app.tatakai.next.model.SOCIAL_NOT_LINKED
import app.tatakai.next.model.SocialActivity
import app.tatakai.next.model.SocialNotification
import app.tatakai.next.model.SocialPage
import app.tatakai.next.model.SocialRepository
import app.tatakai.next.model.SocialUser
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.OkHttpClient
import java.io.File

/** Single activity by id (thread screen, notification targets); not part of the contract interface. Implemented by [SocialRepositoryImpl]. */
fun interface SocialActivityLookup {
    suspend fun activityById(id: Int): SocialActivity?
}

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.7, worker t; checklist social.*, details.reviews): [SocialRepository]
 * over `https://graphql.anilist.co` with `Authorization: Bearer <ListsRepository.accessToken(ANILIST)>`. Queries: `Viewer`, `User(name:)`
 * (+ `statistics`, `favourites`), `Page.followers / following`, `Page.activities(userId / isFollowing, sort: ID_DESC)`, `Page.activityReplies`,
 * `Page.notifications(resetNotificationCount:)`, `Viewer.unreadNotificationCount`, `Page.reviews(mediaId, sort: RATING_DESC)`; mutations
 * `ToggleFollow`, `ToggleLikeV2`, `SaveActivityReply`, `SaveTextActivity`. Reads cached 60 s in memory (+ the viewer in `cacheDir/viewer.json`
 * for the offline You card). `viewer` follows `ListsRepository.accounts` (null when AniList is not linked). Without a token every call
 * throws `AppException(AppError.Unknown(SOCIAL_NOT_LINKED))` (reviews are public and also load without an account). Rate limit
 * (90 req/min) honoured with `Retry-After` ([AniListSocialApi]).
 */
class SocialRepositoryImpl(
    private val client: OkHttpClient,
    private val lists: ListsRepository,
    private val cacheDir: File,
    private val notificationsEnabled: () -> Boolean = { true },
    private val clock: () -> Long = System::currentTimeMillis,
    // --- private additions of worker t (defaults keep the contract constructor) ---
    private val texts: SocialTexts = EnglishSocialTexts,
    private val api: AniListSocialApi = AniListSocialApi(client),
    private val io: CoroutineDispatcher = Dispatchers.IO,
    /** False in tests: do not observe `ListsRepository.accounts` on construction. */
    observeAccounts: Boolean = true,
) : SocialRepository, AppDataWiper, SocialActivityLookup {
    private val scope = CoroutineScope(SupervisorJob() + io)
    private val _viewer = MutableStateFlow<SocialUser?>(null)
    override val viewer: StateFlow<SocialUser?> = _viewer.asStateFlow()
    private val _unread = MutableStateFlow(0)
    override val unreadCount: StateFlow<Int> = _unread.asStateFlow()

    private val cacheLock = Mutex()
    private val cache = HashMap<String, Pair<Long, Any?>>()
    private val json = Json { ignoreUnknownKeys = true }

    /** Whether notifications should be polled (Settings › Listas y cuentas › AniList notifications). */
    val pollingEnabled: Boolean get() = notificationsEnabled()

    init {
        if (observeAccounts) {
            scope.launch {
                readStoredViewer()?.let { _viewer.value = it }
                lists.accounts.map { list -> list.firstOrNull { it.provider == ListProvider.ANILIST }?.userId }.distinctUntilChanged().collect { linked ->
                    if (linked == null) {
                        _viewer.value = null
                        _unread.value = 0
                        clearCache()
                        withContext(io) { File(cacheDir, VIEWER_FILE).delete() }
                    } else {
                        runCatchingNonCancel { profile(null) }
                        runCatchingNonCancel { refreshUnread() }
                    }
                }
            }
        }
    }

    // ---- profile & follows -------------------------------------------------------------------------------------------------------

    override suspend fun profile(userName: String?): SocialUser? {
        val token = token()
        val own = userName == null || userName.equals(_viewer.value?.name, ignoreCase = true)
        val key = if (own) "profile:" else "profile:${userName.lowercase()}"
        return cached(key) {
            val data = if (own) api.post(AniListSocialApi.VIEWER, EMPTY, token) else api.post(AniListSocialApi.USER, vars { put("name", userName) }, token)
            val node = data.obj(if (own) "Viewer" else "User") ?: return@cached null
            val id = node.int("id") ?: return@cached null
            val counts = runCatchingNonCancel { api.post(AniListSocialApi.FOLLOW_COUNTS, vars { put("id", id) }, token) }
            val followers = counts?.obj("followers")?.obj("pageInfo")?.int("total") ?: 0
            val following = counts?.obj("following")?.obj("pageInfo")?.int("total") ?: 0
            val viewerId = if (own) id else _viewer.value?.id
            val user = SocialParsers.user(node, viewerId, followers, following)
            if (own && user != null) {
                _viewer.value = user
                node.int("unreadNotificationCount")?.let { _unread.value = it }
                storeViewer(user)
            }
            user
        }
    }

    override suspend fun followers(userId: Int, page: Int): SocialPage<SocialUser> = followPage(AniListSocialApi.FOLLOWERS, "followers", userId, page)

    override suspend fun following(userId: Int, page: Int): SocialPage<SocialUser> = followPage(AniListSocialApi.FOLLOWING, "following", userId, page)

    private suspend fun followPage(query: String, key: String, userId: Int, page: Int): SocialPage<SocialUser> {
        val token = token()
        return cached("$key:$userId:$page") {
            val data = api.post(query, vars { put("id", userId); put("page", page) }, token)
            SocialParsers.page(data, key, page) { SocialParsers.user(it, _viewer.value?.id) }
        }
    }

    override suspend fun toggleFollow(userId: Int): Result<Boolean> = mutate {
        val data = api.post(AniListSocialApi.TOGGLE_FOLLOW, vars { put("id", userId) }, token())
        data.obj("ToggleFollow")?.bool("isFollowing") ?: throw AppException(AppError.Http(502))
    }

    // ---- activity ----------------------------------------------------------------------------------------------------------------

    override suspend fun activity(userId: Int?, followingOnly: Boolean, page: Int): SocialPage<SocialActivity> {
        val token = token()
        val target = if (followingOnly) null else (userId ?: viewerId(token))
        return cached("activity:$target:$followingOnly:$page") {
            val data = api.post(
                AniListSocialApi.ACTIVITIES,
                vars {
                    if (target != null) put("userId", target)
                    if (followingOnly) put("following", true)
                    put("page", page)
                },
                token,
            )
            SocialParsers.page(data, "activities", page) { SocialParsers.activity(it, _viewer.value?.id) }
        }
    }

    override suspend fun activityById(id: Int): SocialActivity? {
        val token = token()
        return cached("activity-one:$id") {
            SocialParsers.activity(api.post(AniListSocialApi.ACTIVITY, vars { put("id", id) }, token).obj("Activity"), _viewer.value?.id)
        }
    }

    override suspend fun replies(activityId: Int, page: Int): SocialPage<ActivityReply> {
        val token = token()
        return cached("replies:$activityId:$page") {
            val data = api.post(AniListSocialApi.REPLIES, vars { put("id", activityId); put("page", page) }, token)
            SocialParsers.page(data, "activityReplies", page) { SocialParsers.reply(it, _viewer.value?.id) }
        }
    }

    override suspend fun toggleLike(activityId: Int): Result<Boolean> = mutate {
        val data = api.post(AniListSocialApi.TOGGLE_LIKE, vars { put("id", activityId) }, token())
        data.obj("ToggleLikeV2")?.bool("isLiked") ?: throw AppException(AppError.Http(502))
    }

    override suspend fun reply(activityId: Int, text: String): Result<ActivityReply> = mutate {
        val clean = text.trim().also { require(it.isNotEmpty()) { "empty reply" } }
        val data = api.post(AniListSocialApi.SAVE_REPLY, vars { put("id", activityId); put("text", clean) }, token())
        SocialParsers.reply(data.obj("SaveActivityReply"), _viewer.value?.id) ?: throw AppException(AppError.Http(502))
    }

    override suspend fun postStatus(text: String): Result<SocialActivity> = mutate {
        val clean = text.trim().also { require(it.isNotEmpty()) { "empty status" } }
        val data = api.post(AniListSocialApi.SAVE_TEXT, vars { put("text", clean) }, token())
        SocialParsers.activity(data.obj("SaveTextActivity"), _viewer.value?.id) ?: throw AppException(AppError.Http(502))
    }

    // ---- notifications -----------------------------------------------------------------------------------------------------------

    override suspend fun notifications(page: Int, markRead: Boolean): SocialPage<SocialNotification> {
        val token = token()
        // AniList has no per-row read flag: the first `unreadNotificationCount` rows (before the reset) are the new ones.
        val unreadBefore = if (page == 1) {
            runCatchingNonCancel { api.post(AniListSocialApi.UNREAD, EMPTY, token).obj("Viewer")?.int("unreadNotificationCount") } ?: _unread.value
        } else {
            lastUnreadBefore
        }
        if (page == 1) lastUnreadBefore = unreadBefore
        val data = api.post(AniListSocialApi.NOTIFICATIONS, vars { put("page", page); put("reset", markRead) }, token)
        val offset = (page - 1) * NOTIFICATIONS_PER_PAGE
        var index = 0
        val result = SocialParsers.page(data, "notifications", page) { node ->
            val unread = offset + index < unreadBefore
            index++
            SocialParsers.notification(node, _viewer.value?.id, texts, unread)
        }
        _unread.value = if (markRead) 0 else unreadBefore
        return result
    }

    @Volatile
    private var lastUnreadBefore: Int = 0

    override suspend fun refreshUnread() {
        val token = lists.accessToken(ListProvider.ANILIST)
        if (token == null) {
            _unread.value = 0
            return
        }
        val count = api.post(AniListSocialApi.UNREAD, EMPTY, token).obj("Viewer")?.int("unreadNotificationCount") ?: return
        _unread.value = count
    }

    // ---- details slot --------------------------------------------------------------------------------------------------------------

    /** Reviews are public on AniList: they load with the viewer's token when there is one and anonymously otherwise. */
    override suspend fun reviews(ref: MediaRef, page: Int): SocialPage<Review> {
        val id = ref.anilistId ?: return SocialPage(emptyList(), page, false)
        val token = lists.accessToken(ListProvider.ANILIST)
        return cached("reviews:$id:$page") {
            val data = api.post(AniListSocialApi.REVIEWS, vars { put("mediaId", id); put("page", page) }, token)
            SocialParsers.page(data, "reviews", page) { SocialParsers.review(it, _viewer.value?.id) }
        }
    }

    override suspend fun mediaActivity(ref: MediaRef): List<SocialActivity> {
        val id = ref.anilistId ?: return emptyList()
        val token = lists.accessToken(ListProvider.ANILIST) ?: return emptyList()
        val viewerId = viewerId(token)
        return cached("media-activity:$viewerId:$id") {
            val data = api.post(AniListSocialApi.ACTIVITIES, vars { put("userId", viewerId); put("mediaId", id); put("page", 1) }, token)
            SocialParsers.page(data, "activities", 1) { SocialParsers.activity(it, viewerId) }.items
        }
    }

    // ---- reset ---------------------------------------------------------------------------------------------------------------------

    /** "Restablecer la app": cached viewer and pages. */
    override suspend fun wipe() {
        _viewer.value = null
        _unread.value = 0
        clearCache()
        withContext(io) { cacheDir.deleteRecursively() }
    }

    // ---- plumbing ------------------------------------------------------------------------------------------------------------------

    private suspend fun token(): String = lists.accessToken(ListProvider.ANILIST) ?: throw notLinkedException()

    private suspend fun viewerId(token: String): Int {
        _viewer.value?.id?.let { return it }
        val data = api.post(AniListSocialApi.UNREAD, EMPTY, token)
        return data.obj("Viewer")?.int("id") ?: throw notLinkedException()
    }

    @Suppress("UNCHECKED_CAST")
    private suspend fun <T> cached(key: String, load: suspend () -> T): T {
        val now = clock()
        cacheLock.withLock { cache[key]?.takeIf { now - it.first < CACHE_TTL_MS }?.let { return it.second as T } }
        val value = load()
        cacheLock.withLock {
            cache[key] = clock() to value
            if (cache.size > MAX_CACHE_ENTRIES) cache.keys.take(cache.size - MAX_CACHE_ENTRIES).forEach { cache.remove(it) }
        }
        return value
    }

    private suspend fun clearCache() = cacheLock.withLock { cache.clear() }

    /** A write: clears the read cache (counts, liked flags and threads change) and maps failures into the Result. */
    private suspend fun <T> mutate(block: suspend () -> T): Result<T> = try {
        val v = block()
        clearCache()
        Result.success(v)
    } catch (e: CancellationException) {
        throw e
    } catch (e: Exception) {
        Result.failure(e)
    }

    private suspend fun readStoredViewer(): SocialUser? = withContext(io) {
        runCatching { json.decodeFromString(StoredViewer.serializer(), File(cacheDir, VIEWER_FILE).readText()).toUser() }.getOrNull()
    }

    private suspend fun storeViewer(user: SocialUser) {
        withContext(io) {
            runCatching {
                cacheDir.mkdirs()
                File(cacheDir, VIEWER_FILE).writeText(json.encodeToString(StoredViewer.serializer(), StoredViewer.of(user)))
            }
        }
    }

    private suspend fun <T> runCatchingNonCancel(block: suspend () -> T): T? = try {
        block()
    } catch (e: CancellationException) {
        throw e
    } catch (_: Exception) {
        null
    }

    private fun notLinkedException() = AppException(AppError.Unknown(SOCIAL_NOT_LINKED))

    private inline fun vars(block: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit): JsonObject = buildJsonObject(block)

    /** The few viewer fields kept on disk for the offline You card (no token, no stats). */
    @Serializable
    private data class StoredViewer(val id: Int, val name: String, val avatar: String? = null, val banner: String? = null, val siteUrl: String) {
        fun toUser() = SocialUser(id = id, name = name, avatar = avatar, banner = banner, isViewer = true, siteUrl = siteUrl)

        companion object {
            fun of(u: SocialUser) = StoredViewer(u.id, u.name, u.avatar, u.banner, u.siteUrl)
        }
    }

    companion object {
        const val CACHE_DIR: String = "social"
        const val GRAPHQL_URL: String = "https://graphql.anilist.co"

        /** Notification channel of [SocialNotificationsWorker]. */
        const val CHANNEL_ID: String = "social"

        const val CACHE_TTL_MS: Long = 60_000
        const val NOTIFICATIONS_PER_PAGE: Int = 25
        private const val MAX_CACHE_ENTRIES = 200
        private const val VIEWER_FILE = "viewer.json"
        private val EMPTY = JsonObject(emptyMap())
    }
}
