// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import app.cash.turbine.test
import app.tatakai.next.data.InitialSyncMode
import app.tatakai.next.data.ListsSyncControl
import app.tatakai.next.model.AppSettings
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
import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

/** parity wave A closure (worker q, lists.import-export): the import remote / export local / merge choice when linking. */
@OptIn(ExperimentalCoroutinesApi::class)
class AccountsSyncModeTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private class FakeSettings : SettingsRepository {
        override val settings = MutableStateFlow(AppSettings())
        override suspend fun update(transform: (AppSettings) -> AppSettings) {
            settings.value = transform(settings.value)
        }
    }

    private class FakeLists : ListsRepository, ListsSyncControl {
        val calls = mutableListOf<Pair<ListProvider, InitialSyncMode>>()
        override val accounts = MutableStateFlow(listOf(LinkedAccount(ListProvider.ANILIST, "1", "yuki", null, 0L, null)))
        override val pendingChanges: StateFlow<Int> = MutableStateFlow(0)
        override val lastSyncedAt: StateFlow<Long?> = MutableStateFlow<Long?>(null)
        override val firstSyncPending = MutableStateFlow(setOf(ListProvider.ANILIST))
        override val scoreFormat: StateFlow<String?> = MutableStateFlow<String?>(null)
        override suspend fun syncWith(provider: ListProvider, mode: InitialSyncMode): Result<ListSyncReport> {
            calls += provider to mode
            firstSyncPending.value = firstSyncPending.value - provider
            return Result.success(ListSyncReport(pushed = 1, pulled = 2, conflicts = 0, failed = 0))
        }
        override suspend fun pushPending(): ListSyncReport = ListSyncReport(0, 0, 0, 0)
        override fun entries(status: ListStatus?): Flow<List<ListEntry>> = flowOf(emptyList())
        override fun favorites(): Flow<List<ListEntry>> = flowOf(emptyList())
        override fun entry(ref: MediaRef): Flow<ListEntry?> = flowOf(null)
        override suspend fun setStatus(media: Media, status: ListStatus?): Result<Unit> = Result.success(Unit)
        override suspend fun setProgress(ref: MediaRef, episode: Int): Result<Unit> = Result.success(Unit)
        override suspend fun setScore(ref: MediaRef, score: Int?): Result<Unit> = Result.success(Unit)
        override suspend fun setFavorite(media: Media, favorite: Boolean): Result<Unit> = Result.success(Unit)
        override fun favoriteCharacters(): Flow<List<CharacterRef>> = flowOf(emptyList())
        override fun isCharacterFavorite(characterId: Int): Flow<Boolean> = flowOf(false)
        override suspend fun setCharacterFavorite(character: CharacterRef, favorite: Boolean): Result<Unit> = Result.success(Unit)
        override suspend fun beginLink(provider: ListProvider): Result<String> = Result.failure(UnsupportedOperationException())
        override suspend fun completeLink(redirectUri: String): Result<LinkedAccount> = Result.failure(UnsupportedOperationException())
        override suspend fun unlink(provider: ListProvider) {}
        override suspend fun syncNow(): Result<ListSyncReport> = Result.success(ListSyncReport(0, 0, 0, 0))
        override fun conflicts(): Flow<List<ListConflict>> = flowOf(emptyList())
        override suspend fun resolveConflict(id: String, choice: ConflictChoice) {}
        override suspend fun exportJson(): String = "[]"
        override suspend fun importJson(json: String): Result<Int> = Result.success(0)
    }

    @Test
    fun `sheet opens for a pending first sync, the reopened provider first, and never while syncing`() {
        val pending = setOf(ListProvider.MAL)
        assertEquals(ListProvider.MAL, syncModeSheetOf(asking = null, firstSyncPending = pending, dismissed = emptySet(), syncing = false))
        assertNull(syncModeSheetOf(asking = null, firstSyncPending = pending, dismissed = pending, syncing = false))
        assertEquals(ListProvider.ANILIST, syncModeSheetOf(ListProvider.ANILIST, pending, emptySet(), syncing = false))
        assertNull(syncModeSheetOf(ListProvider.ANILIST, pending, emptySet(), syncing = true))
        assertNull(syncModeSheetOf(null, emptySet(), emptySet(), syncing = false))
    }

    @Test
    fun `choosing import runs that mode and closes the sheet`() = runTest {
        val lists = FakeLists()
        val vm = AccountsViewModel(lists, FakeSettings())
        assertEquals(ListProvider.ANILIST, vm.state.first { it.syncModeSheet != null }.syncModeSheet)

        vm.events.test {
            vm.syncWithMode(ListProvider.ANILIST, InitialSyncMode.IMPORT)
            assertEquals(AccountsEvent.Synced(ListSyncReport(1, 2, 0, 0)), awaitItem())
        }
        assertEquals(listOf(ListProvider.ANILIST to InitialSyncMode.IMPORT), lists.calls)
        val after = vm.state.first { it.firstSyncPending.isEmpty() }
        assertNull(after.syncModeSheet)
    }

    @Test
    fun `later keeps the first sync pending and the card reopens the sheet for export`() = runTest {
        val lists = FakeLists()
        val vm = AccountsViewModel(lists, FakeSettings())
        vm.state.first { it.syncModeSheet != null }

        vm.dismissSyncMode()
        val closed = vm.state.first { it.syncModeSheet == null }
        assertEquals(setOf(ListProvider.ANILIST), closed.firstSyncPending)
        assertEquals(emptyList(), lists.calls)

        vm.askSyncMode(ListProvider.ANILIST)
        assertEquals(ListProvider.ANILIST, vm.state.first { it.syncModeSheet != null }.syncModeSheet)
        vm.syncWithMode(ListProvider.ANILIST, InitialSyncMode.EXPORT)
        assertEquals(listOf(ListProvider.ANILIST to InitialSyncMode.EXPORT), lists.calls)
    }

    @Test
    fun `an unlinked provider cannot open the sheet`() = runTest {
        val lists = FakeLists().apply { firstSyncPending.value = emptySet() }
        val vm = AccountsViewModel(lists, FakeSettings())
        vm.askSyncMode(ListProvider.MAL)
        assertNull(vm.state.first().syncModeSheet)
    }
}
