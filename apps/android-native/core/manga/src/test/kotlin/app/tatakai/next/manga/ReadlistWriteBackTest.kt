// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

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
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import java.nio.file.Files
import java.util.Collections
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** "Enviar los cambios a las cuentas" off: manga / novel read list edits stay on the phone; pulls still happen. Never the real AniList. */
class ReadlistWriteBackTest {

    private class FakeLists : ListsRepository {
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
        override suspend fun accessToken(provider: ListProvider): String? = if (provider == ListProvider.ANILIST) "test-token" else null
    }

    private val manga = Media(
        ref = MediaRef(anilistId = 42), kind = MediaKind.MANGA, title = "Holes", titles = emptyMap(), cover = null, banner = null, color = null,
        synopsis = null, genres = emptyList(), episodes = 20, status = null, season = null, year = null, score = null, sourceProvider = "test",
    )

    private fun withServer(block: suspend (MockWebServer, MutableList<String>) -> Unit) {
        val bodies = Collections.synchronizedList(mutableListOf<String>())
        val server = MockWebServer().apply {
            dispatcher = object : Dispatcher() {
                override fun dispatch(request: RecordedRequest): MockResponse {
                    val body = request.body?.utf8().orEmpty()
                    bodies += body
                    val json = if ("Viewer" in body) """{"data":{"Viewer":{"id":7}}}""" else """{"data":{}}"""
                    return MockResponse.Builder().code(200).addHeader("Content-Type", "application/json").body(json).build()
                }
            }
            start()
        }
        try {
            runBlocking { block(server, bodies) }
        } finally {
            server.close()
        }
    }

    private fun store(server: MockWebServer, writeBack: Boolean) = ReadlistStore(
        file = Files.createTempDirectory("readlist").resolve("readlist.json").toFile(),
        anilist = AniListMangaApi(OkHttpClient(), server.url("/").toString()),
        lists = FakeLists(),
        novels = false,
        clock = { 1_000L },
        writeBack = { writeBack },
    )

    @Test
    fun writeBackOffKeepsEditsLocalAndStillPulls() = withServer { server, bodies ->
        val store = store(server, writeBack = false)
        store.setStatus(manga, ListStatus.WATCHING).getOrThrow()
        store.setProgress(manga.ref, 5).getOrThrow()
        assertTrue(bodies.none { "SaveMediaListEntry" in it || "DeleteMediaListEntry" in it }, "nothing pushed: $bodies")
        assertEquals(5, store.entry(manga.ref).first()?.progress, "the edit is kept locally")

        store.pull().getOrThrow()
        assertTrue(bodies.any { "MediaListCollection" in it }, "the AniList read list is still pulled")
        assertTrue(bodies.none { "SaveMediaListEntry" in it }, "no pending push either")
    }

    @Test
    fun writeBackOnPushesTheEdit() = withServer { server, bodies ->
        val store = store(server, writeBack = true)
        store.setStatus(manga, ListStatus.WATCHING).getOrThrow()
        store.setProgress(manga.ref, 5).getOrThrow()
        assertEquals(2, bodies.count { "SaveMediaListEntry" in it })
    }
}
