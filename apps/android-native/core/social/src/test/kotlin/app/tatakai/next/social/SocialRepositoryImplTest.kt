// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.social

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.ConflictChoice
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListConflict
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListSyncReport
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.SOCIAL_NOT_LINKED
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.runBlocking
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import java.nio.file.Files
import java.util.concurrent.atomic.AtomicInteger
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertIs
import kotlin.test.assertTrue

// parity wave B (16 §5 t.5g, worker t): repository behaviour against a fake AniList (MockWebServer).
class SocialRepositoryImplTest {
    private class FakeLists(var token: String?) : ListsRepository {
        override val accounts: StateFlow<List<LinkedAccount>> = MutableStateFlow(emptyList())
        override val pendingChanges: StateFlow<Int> = MutableStateFlow(0)
        override val lastSyncedAt: StateFlow<Long?> = MutableStateFlow(null)
        override fun entries(status: ListStatus?): Flow<List<ListEntry>> = emptyFlow()
        override fun favorites(): Flow<List<ListEntry>> = emptyFlow()
        override fun entry(ref: MediaRef): Flow<ListEntry?> = emptyFlow()
        override suspend fun setStatus(media: Media, status: ListStatus?): Result<Unit> = Result.success(Unit)
        override suspend fun setProgress(ref: MediaRef, episode: Int): Result<Unit> = Result.success(Unit)
        override suspend fun setScore(ref: MediaRef, score: Int?): Result<Unit> = Result.success(Unit)
        override suspend fun setFavorite(media: Media, favorite: Boolean): Result<Unit> = Result.success(Unit)
        override fun favoriteCharacters(): Flow<List<CharacterRef>> = emptyFlow()
        override fun isCharacterFavorite(characterId: Int): Flow<Boolean> = emptyFlow()
        override suspend fun setCharacterFavorite(character: CharacterRef, favorite: Boolean): Result<Unit> = Result.success(Unit)
        override suspend fun beginLink(provider: ListProvider): Result<String> = Result.failure(UnsupportedOperationException())
        override suspend fun completeLink(redirectUri: String): Result<LinkedAccount> = Result.failure(UnsupportedOperationException())
        override suspend fun unlink(provider: ListProvider) {}
        override suspend fun syncNow(): Result<ListSyncReport> = Result.failure(UnsupportedOperationException())
        override fun conflicts(): Flow<List<ListConflict>> = emptyFlow()
        override suspend fun resolveConflict(id: String, choice: ConflictChoice) {}
        override suspend fun exportJson(): String = "[]"
        override suspend fun importJson(json: String): Result<Int> = Result.success(0)
        override suspend fun accessToken(provider: ListProvider): String? = if (provider == ListProvider.ANILIST) token else null
    }

    private fun fixture(name: String) = checkNotNull(javaClass.getResource("/anilist/$name")).readText()

    private fun json(body: String, code: Int = 200) = MockResponse.Builder().code(code).addHeader("Content-Type", "application/json").body(body).build()

    private fun withServer(handler: (RecordedRequest) -> MockResponse, block: suspend (MockWebServer) -> Unit) {
        val server = MockWebServer().apply {
            dispatcher = object : Dispatcher() {
                override fun dispatch(request: RecordedRequest): MockResponse = handler(request)
            }
            start()
        }
        try {
            runBlocking { block(server) }
        } finally {
            server.close()
        }
    }

    private fun repo(server: MockWebServer, lists: ListsRepository, clock: () -> Long = { 0L }) = SocialRepositoryImpl(
        client = OkHttpClient(),
        lists = lists,
        cacheDir = Files.createTempDirectory("social").toFile(),
        clock = clock,
        api = AniListSocialApi(OkHttpClient(), server.url("/").toString(), maxRetryWaitSec = 1),
        observeAccounts = false,
    )

    @Test
    fun withoutTokenEveryCallIsNotLinked() = withServer({ json("{}") }) { server ->
        val r = repo(server, FakeLists(null))
        val e = assertFailsWith<AppException> { r.profile() }
        assertEquals(AppError.Unknown(SOCIAL_NOT_LINKED), e.error)
        assertFailsWith<AppException> { r.notifications() }
        assertTrue(r.toggleFollow(1).isFailure)
        assertTrue(r.mediaActivity(MediaRef(anilistId = 1)).isEmpty())
        assertEquals(0, server.requestCount)
    }

    @Test
    fun readsAreCachedForSixtySeconds() {
        val hits = AtomicInteger()
        var now = 0L
        withServer({ req ->
            val body = req.body?.utf8().orEmpty()
            when {
                "reviews" in body -> {
                    hits.incrementAndGet()
                    json(fixture("reviews.json"))
                }
                else -> json("""{"data":{}}""")
            }
        }) { server ->
            val r = repo(server, FakeLists("tok"), clock = { now })
            r.reviews(MediaRef(anilistId = 154587))
            now = 59_000
            r.reviews(MediaRef(anilistId = 154587))
            assertEquals(1, hits.get())
            now = 61_000
            val page = r.reviews(MediaRef(anilistId = 154587))
            assertEquals(2, hits.get())
            assertTrue(page.hasNext)
            assertEquals("Bearer tok", server.takeRequest().headers["Authorization"])
        }
    }

    @Test
    fun reviewsLoadWithoutAnAccount() = withServer({ json(fixture("reviews.json")) }) { server ->
        val r = repo(server, FakeLists(null))
        assertEquals(1, r.reviews(MediaRef(anilistId = 1)).items.size)
        assertEquals(null, server.takeRequest().headers["Authorization"])
    }

    @Test
    fun retryAfterIsHonouredOnce() {
        val calls = AtomicInteger()
        withServer({
            if (calls.incrementAndGet() == 1) {
                MockResponse.Builder().code(429).addHeader("Retry-After", "1").body("""{"errors":[{"status":429}]}""").build()
            } else {
                json(fixture("viewer.json"))
            }
        }) { server ->
            val r = repo(server, FakeLists("tok"))
            val u = r.profile()
            assertEquals("Octa", u?.name)
            assertEquals(3, r.unreadCount.value)
            assertEquals(r.viewer.value?.id, 5001)
            assertTrue(calls.get() >= 2)
        }
    }

    @Test
    fun rateLimitSurfacesAfterTheRetry() = withServer({
        MockResponse.Builder().code(429).addHeader("Retry-After", "1").body("{}").build()
    }) { server ->
        val r = repo(server, FakeLists("tok"))
        val e = assertFailsWith<AppException> { r.profile() }
        assertIs<AppError.RateLimited>(e.error)
    }

    @Test
    fun notificationsMarkTheFirstUnreadRowsAndReset() = withServer({ req ->
        val body = req.body?.utf8().orEmpty()
        if ("notifications(" in body) json(fixture("notifications.json")) else json("""{"data":{"Viewer":{"id":5001,"unreadNotificationCount":2}}}""")
    }) { server ->
        val r = repo(server, FakeLists("tok"))
        val page = r.notifications(page = 1, markRead = true)
        assertEquals(listOf(true, true, false, false, false), page.items.map { it.unread })
        assertEquals(0, r.unreadCount.value)
        r.refreshUnread()
        assertEquals(2, r.unreadCount.value)
    }
}
