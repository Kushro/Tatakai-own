// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionPrefType
import app.tatakai.next.model.ExtensionPreference
import app.tatakai.next.model.ExtensionRepo
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.ExtensionUpdate
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.test.StandardTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class ExtensionsHubViewModelTest {
    private val dispatcher = StandardTestDispatcher()

    @BeforeTest fun setUp() = Dispatchers.setMain(dispatcher)

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private class FakeRepo : ExtensionRepository {
        override val installed = MutableStateFlow(listOf(InstalledExtension("a", "A", "1.0.0", "custom", emptyList(), ExtensionCompat.FULL, true)))
        override suspend fun installFromUri(uri: String): Result<InstalledExtension> = Result.failure(IllegalStateException())
        override suspend fun setEnabled(id: String, enabled: Boolean) {}
        override suspend fun remove(id: String) {}
        override suspend fun resolveStreams(media: Media, episode: Int): List<StreamSource> = emptyList()

        val upd = MutableStateFlow(listOf(ExtensionUpdate("a", "A", "1.0.0", "1.1.0", "https://r.example/a.kai", null, "https://r.example/index.json")))
        override val updates: StateFlow<List<ExtensionUpdate>> get() = upd
        val repos = MutableStateFlow<List<ExtensionRepo>>(emptyList())
        override val repositories: StateFlow<List<ExtensionRepo>> get() = repos
        var allEnabled: Boolean? = null
        override suspend fun setAllEnabled(enabled: Boolean) {
            allEnabled = enabled
            installed.value = installed.value.map { it.copy(enabled = enabled) }
        }
        override suspend fun update(extensionId: String): Result<InstalledExtension> {
            upd.value = upd.value.filterNot { it.id == extensionId }
            return Result.success(installed.value.first { it.id == extensionId }.copy(version = "1.1.0"))
        }
        override suspend fun customSources(kind: String?): List<CustomSourceRef> = listOf(CustomSourceRef("a", "A", "main", "watch", "Main"))
        override suspend fun preferences(extensionId: String): List<ExtensionPreference> =
            if (extensionId == "a") listOf(ExtensionPreference("lang", "Language", ExtensionPrefType.SELECT, options = listOf("en", "es"))) else emptyList()
        override suspend fun addRepository(url: String): Result<ExtensionRepo> {
            if (!url.endsWith("index.json")) return Result.failure(IllegalArgumentException("not an index"))
            val r = ExtensionRepo(url, "Repo", emptyList(), 1L)
            repos.value = repos.value + r
            return Result.success(r)
        }
    }

    @Test
    fun loadsProvidersPreferenceOwnersAndUpdates() = runTest(dispatcher) {
        val repo = FakeRepo()
        val vm = ExtensionsHubViewModel(repo)
        val job = launch { vm.state.collect {} }
        advanceUntilIdle()
        val s = vm.state.value
        assertEquals(1, s.updates.size)
        assertEquals(setOf("a"), s.withPreferences)
        val providers = assertIs<LoadState.Ok<List<CustomSourceRef>>>(s.providers)
        assertEquals("main", providers.value.single().sourceId)
        vm.update("a")
        advanceUntilIdle()
        assertTrue(vm.state.value.updates.isEmpty())
        assertIs<HubNotice.Updated>(vm.notices.first())
        job.cancel()
    }

    @Test
    fun killSwitchAndRepositoryErrors() = runTest(dispatcher) {
        val repo = FakeRepo()
        val vm = ExtensionsHubViewModel(repo)
        val job = launch { vm.state.collect {} }
        vm.setAllEnabled(false)
        advanceUntilIdle()
        assertEquals(false, repo.allEnabled)
        assertTrue(repo.installed.value.none { it.enabled })
        assertEquals(HubNotice.AllEnabled(false), vm.notices.first())
        vm.addRepository("https://r.example/nope")
        advanceUntilIdle()
        assertEquals("not an index", vm.state.value.addRepoError)
        vm.addRepository("https://r.example/index.json")
        advanceUntilIdle()
        assertEquals(1, vm.state.value.repos.size)
        assertEquals(null, vm.state.value.addRepoError)
        job.cancel()
    }
}
