// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.FavoriteCharacterRow
import app.tatakai.next.database.LinkedAccountRow
import app.tatakai.next.database.ListChangeRow
import app.tatakai.next.database.ListConflictRow
import app.tatakai.next.database.ListEntryRow
import app.tatakai.next.database.ListsDao
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ReadListPuller
import app.tatakai.next.model.SecretKeys
import app.tatakai.next.model.SecretStore
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.runBlocking
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/** "Sincronizar ahora" pulls the manga / novel read lists on EVERY call (not once per process) and counts what they received. */
class ListsReadListSyncTest {

    private lateinit var server: MockWebServer
    private val bodies = java.util.Collections.synchronizedList(mutableListOf<String>())
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Unconfined)

    @BeforeTest
    fun setUp() {
        // every AniList query answers an empty `data`: empty anime list, no favorites (never the real AniList)
        server = MockWebServer().apply {
            dispatcher = object : Dispatcher() {
                override fun dispatch(request: RecordedRequest): MockResponse {
                    bodies += request.body?.utf8().orEmpty()
                    return MockResponse.Builder().code(200).body("""{"data":{}}""").build()
                }
            }
            start()
        }
    }

    @AfterTest
    fun tearDown() {
        scope.cancel()
        server.close()
    }

    private class CountingPuller(private val result: () -> Result<Int>) : ReadListPuller {
        var calls = 0
        override suspend fun pullReadList(): Result<Int> {
            calls++
            return result()
        }
    }

    private var pushRequests = 0

    private fun repo(
        dao: FakeListsDao,
        pullers: List<ReadListPuller>,
        secrets: SecretStore = FakeSecrets(),
        writeBack: Boolean = true,
    ): ListsRepositoryImpl =
        ListsRepositoryImpl(
            dao = dao,
            secrets = secrets,
            anilist = AniListListClient(OkHttpClient(), server.url("/").toString()),
            mal = MalListClient(OkHttpClient(), server.url("/mal").toString()),
            clientIds = { "id" },
            clock = { 1_000L },
            scope = scope,
            syncOnStart = false,
            readLists = { pullers },
            scheduler = { pushRequests++ },
            writeBack = { writeBack },
        )

    private suspend fun linkedSecrets(): FakeSecrets = FakeSecrets().apply {
        put(SecretKeys.listToken(ListProvider.ANILIST.id), listJson.encodeToString(TokenSet.serializer(), TokenSet(access = "test-token")))
    }

    private fun anilistAccount() = LinkedAccountRow(ListProvider.ANILIST.id, "7", "yuki", null, linkedAtMs = 0L, expiresAtMs = null, lastSyncedAtMs = 500L)

    @Test
    fun syncNowPullsEveryReadListOnEveryCall() = runBlocking {
        val dao = FakeListsDao().apply { putAccount(anilistAccount()) }
        val manga = CountingPuller { Result.success(3) }
        val novels = CountingPuller { Result.success(2) }
        val lists = repo(dao, listOf(manga, novels), linkedSecrets())

        val first = lists.syncNow().getOrThrow()
        val second = lists.syncNow().getOrThrow()

        assertEquals(2, manga.calls)
        assertEquals(2, novels.calls)
        assertEquals(5, first.pulled, "anime list empty: the received count is the read lists' 3 + 2")
        assertEquals(5, second.pulled)
        assertEquals(0, first.failed)
    }

    @Test
    fun aFailingReadListIsCountedAndDoesNotStopTheOthers() = runBlocking {
        val dao = FakeListsDao().apply { putAccount(anilistAccount()) }
        val broken = CountingPuller { Result.failure(IllegalStateException("offline")) }
        val novels = CountingPuller { Result.success(1) }
        val report = repo(dao, listOf(broken, novels), linkedSecrets()).syncNow().getOrThrow()
        assertEquals(1, broken.calls)
        assertEquals(1, novels.calls)
        assertEquals(1, report.pulled)
        assertEquals(1, report.failed)
    }

    private val media = app.tatakai.next.model.Media(
        ref = app.tatakai.next.model.MediaRef(anilistId = 1), kind = app.tatakai.next.model.MediaKind.ANIME, title = "A", titles = emptyMap(),
        cover = null, banner = null, color = null, synopsis = null, genres = emptyList(), episodes = 12, status = null, season = null,
        year = null, score = null, sourceProvider = "test",
    )

    @Test
    fun writeBackOffQueuesAndPushesNothing() = runBlocking {
        val dao = FakeListsDao().apply { putAccount(anilistAccount()) }
        val lists = repo(dao, emptyList(), linkedSecrets(), writeBack = false)
        lists.setStatus(media, app.tatakai.next.model.ListStatus.WATCHING).getOrThrow()
        lists.setProgress(media.ref, 3).getOrThrow() // the list editor / the watched toggle
        lists.onEpisodeCompleted(media.ref, 4) // the automatic progress
        lists.setFavorite(media, true).getOrThrow()
        assertEquals(4, dao.get("1")?.progress, "the edits are kept locally")
        assertTrue(dao.changes().isEmpty(), "nothing queued: ${dao.changes()}")
        assertEquals(0, pushRequests)
        assertEquals(0, lists.pushPending().pushed)

        val report = lists.syncNow().getOrThrow()
        assertEquals(0, report.pushed)
        assertTrue(bodies.none { "SaveMediaListEntry" in it || "DeleteMediaListEntry" in it || "ToggleFavourite" in it }, "nothing pushed: $bodies")
        assertTrue(bodies.any { "MediaListCollection" in it }, "the anime list is still pulled")
        assertEquals(4, dao.get("1")?.progress)
    }

    @Test
    fun writeBackOnQueuesAndPushes() = runBlocking {
        val dao = FakeListsDao().apply { putAccount(anilistAccount()) }
        val lists = repo(dao, emptyList(), linkedSecrets(), writeBack = true)
        lists.setStatus(media, app.tatakai.next.model.ListStatus.WATCHING).getOrThrow()
        lists.setProgress(media.ref, 3).getOrThrow()
        assertEquals(1, dao.changes().size, "one coalesced change for AniList")
        assertTrue(pushRequests > 0)
        assertEquals(1, lists.syncNow().getOrThrow().pushed)
        assertTrue(bodies.any { "SaveMediaListEntry" in it })
    }

    @Test
    fun withoutAnAniListAccountNoReadListIsPulled() = runBlocking {
        val manga = CountingPuller { Result.success(3) }
        val report = repo(FakeListsDao(), listOf(manga)).syncNow().getOrThrow()
        assertEquals(0, manga.calls)
        assertEquals(0, report.pulled)
    }

    @Test
    fun firstSyncChoicePendingLeavesTheReadListsAlone() = runBlocking {
        // linked, never synced, with a local list: the import / export / merge choice is pending (lists.import-export)
        val dao = FakeListsDao().apply {
            putAccount(anilistAccount().copy(lastSyncedAtMs = null))
            upsert(ListEntryRow(key = "1", anilistId = 1, malId = null, title = "A", poster = null, status = "watching", updatedAtMs = 1L))
        }
        val manga = CountingPuller { Result.success(3) }
        val lists = repo(dao, listOf(manga), linkedSecrets())
        assertTrue(ListProvider.ANILIST in pendingFirstSyncs(dao.accounts(), dao.all()))
        // let the restore of the pending set run (Unconfined: already done), then sync
        lists.syncNow().getOrThrow()
        assertEquals(0, manga.calls)
    }
}

internal class FakeSecrets : SecretStore {
    private val map = HashMap<String, String>()
    override suspend fun get(key: String): String? = map[key]
    override suspend fun put(key: String, value: String?) {
        if (value == null) map.remove(key) else map[key] = value
    }
    override suspend fun clear() = map.clear()
}

/** In-memory [ListsDao] (no Room). */
internal class FakeListsDao : ListsDao() {
    private val entries = MutableStateFlow<Map<String, ListEntryRow>>(emptyMap())
    private val changeRows = MutableStateFlow<List<ListChangeRow>>(emptyList())
    private val conflictRows = MutableStateFlow<Map<String, ListConflictRow>>(emptyMap())
    private val accountRows = MutableStateFlow<Map<String, LinkedAccountRow>>(emptyMap())
    private val characters = MutableStateFlow<Map<Int, FavoriteCharacterRow>>(emptyMap())

    override fun observeListed(): Flow<List<ListEntryRow>> = entries.map { m -> m.values.filter { it.status != null }.sortedByDescending { it.updatedAtMs } }
    override fun observeByStatus(status: String): Flow<List<ListEntryRow>> = entries.map { m -> m.values.filter { it.status == status } }
    override fun observeFavorites(): Flow<List<ListEntryRow>> = entries.map { m -> m.values.filter { it.favorite } }
    override fun observe(key: String): Flow<ListEntryRow?> = entries.map { it[key] }
    override suspend fun get(key: String): ListEntryRow? = entries.value[key]
    override suspend fun byAniList(anilistId: Int): ListEntryRow? = entries.value.values.firstOrNull { it.anilistId == anilistId }
    override suspend fun byMal(malId: Int): ListEntryRow? = entries.value.values.firstOrNull { it.malId == malId }
    override suspend fun all(): List<ListEntryRow> = entries.value.values.toList()
    override suspend fun upsert(row: ListEntryRow) { entries.value = entries.value + (row.key to row) }
    override suspend fun delete(key: String) { entries.value = entries.value - key }

    override suspend fun changes(): List<ListChangeRow> = changeRows.value.sortedBy { it.createdAtMs }
    override fun observeChangeCount(): Flow<Int> = changeRows.map { it.size }
    override suspend fun putChange(row: ListChangeRow) {
        changeRows.value = changeRows.value.filterNot { it.provider == row.provider && it.key == row.key && it.op == row.op } + row
    }
    override suspend fun deleteChange(provider: String, key: String, op: String) {
        changeRows.value = changeRows.value.filterNot { it.provider == provider && it.key == key && it.op == op }
    }
    override suspend fun deleteChangesOf(provider: String) { changeRows.value = changeRows.value.filterNot { it.provider == provider } }

    override fun observeConflicts(): Flow<List<ListConflictRow>> = conflictRows.map { it.values.toList() }
    override suspend fun conflict(id: String): ListConflictRow? = conflictRows.value[id]
    override suspend fun putConflict(row: ListConflictRow) { conflictRows.value = conflictRows.value + (row.id to row) }
    override suspend fun deleteConflict(id: String) { conflictRows.value = conflictRows.value - id }
    override suspend fun deleteConflictsOf(provider: String) { conflictRows.value = conflictRows.value.filterValues { it.provider != provider } }

    override fun observeAccounts(): Flow<List<LinkedAccountRow>> = accountRows.map { it.values.toList() }
    override suspend fun accounts(): List<LinkedAccountRow> = accountRows.value.values.toList()
    override suspend fun putAccount(row: LinkedAccountRow) { accountRows.value = accountRows.value + (row.provider to row) }
    override suspend fun deleteAccount(provider: String) { accountRows.value = accountRows.value - provider }

    override fun observeCharacters(): Flow<List<FavoriteCharacterRow>> = characters.map { it.values.toList() }
    override fun observeCharacterCount(id: Int): Flow<Int> = characters.map { if (id in it) 1 else 0 }
    override suspend fun character(id: Int): FavoriteCharacterRow? = characters.value[id]
    override suspend fun pendingCharacters(): List<FavoriteCharacterRow> = characters.value.values.filter { it.pendingRemote != null }
    override suspend fun putCharacter(row: FavoriteCharacterRow) { characters.value = characters.value + (row.id to row) }
    override suspend fun deleteCharacter(id: Int) { characters.value = characters.value - id }
    override fun observeVisibleCharacters(): Flow<List<FavoriteCharacterRow>> = characters.map { m -> m.values.filter { it.pendingRemote != false } }
    override fun observeVisibleCharacterCount(id: Int): Flow<Int> = characters.map { m -> if (m[id]?.pendingRemote != false && id in m) 1 else 0 }
}
