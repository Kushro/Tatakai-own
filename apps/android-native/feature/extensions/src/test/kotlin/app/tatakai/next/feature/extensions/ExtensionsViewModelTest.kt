package app.tatakai.next.feature.extensions

import app.tatakai.next.model.ExtLogLevel
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionDetail
import app.tatakai.next.model.ExtensionLogEntry
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.Media
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class ExtensionsViewModelTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private fun ext(id: String, enabled: Boolean = true) =
        InstalledExtension(id, "Ext $id", "1.0.0", "source", listOf("network:domain:example.com"), ExtensionCompat.FULL, enabled)

    private class FakeRepo : ExtensionRepository {
        override val installed = MutableStateFlow<List<InstalledExtension>>(emptyList())
        var installResult: (String) -> Result<InstalledExtension> = { Result.failure(IllegalArgumentException("no")) }
        override suspend fun installFromUri(uri: String): Result<InstalledExtension> =
            installResult(uri).onSuccess { installed.value = installed.value + it }
        override suspend fun setEnabled(id: String, enabled: Boolean) {
            installed.value = installed.value.map { if (it.id == id) it.copy(enabled = enabled) else it }
        }
        override suspend fun remove(id: String) {
            installed.value = installed.value.filterNot { it.id == id }
        }
        override suspend fun resolveStreams(media: Media, episode: Int): List<StreamSource> = emptyList()

        var detailFails = true
        override suspend fun detail(extensionId: String): ExtensionDetail? {
            if (detailFails) throw IllegalStateException("boom")
            return ExtensionDetail(extensionId, listOf("example.com"), "9f2c…b71e", 1L, "onlinestream")
        }

        val logLines = MutableStateFlow(listOf(ExtensionLogEntry(ExtLogLevel.OK, "install", "installed", 1L)))
        override fun logs(extensionId: String): Flow<List<ExtensionLogEntry>> = logLines
    }

    @Test
    fun `successful install lists the extension and reports it`() = runTest {
        val repo = FakeRepo().apply { installResult = { Result.success(ext("a")) } }
        val vm = ExtensionsViewModel(repo)

        vm.install("content://picked/a.kai")

        val s = vm.state.first { it.items.isNotEmpty() && it.message != null }
        assertEquals(listOf("a"), s.items.map { it.id })
        assertEquals(InstallMessage.Installed("Ext a"), s.message)
        assertEquals(false, s.installing)
    }

    @Test
    fun `invalid package shows the validation reason and can be dismissed`() = runTest {
        val repo = FakeRepo().apply { installResult = { Result.failure(IllegalArgumentException("manifest.version missing")) } }
        val vm = ExtensionsViewModel(repo)

        vm.install("content://picked/bad.kai")

        val failed = assertIs<InstallMessage.Failed>(vm.state.first { it.message != null }.message)
        assertEquals("manifest.version missing", failed.reason)
        vm.dismissMessage()
        assertNull(vm.state.first { it.message == null }.message)
    }

    @Test
    fun `enable and remove are forwarded to the repository`() = runTest {
        val repo = FakeRepo().apply { installed.value = listOf(ext("a"), ext("b")) }
        val vm = ExtensionsViewModel(repo)

        vm.setEnabled("a", false)
        vm.remove("b")

        val s = vm.state.first { it.items.size == 1 }
        assertEquals(listOf("a"), s.items.map { it.id })
        assertEquals(false, s.items.single().enabled)
    }

    @Test
    fun `details load per row, fail with retry and logs toggle`() = runTest {
        val repo = FakeRepo().apply { installed.value = listOf(ext("a")) }
        val vm = ExtensionsViewModel(repo)

        assertIs<DetailState.Failed>(vm.state.first { it.details["a"] is DetailState.Failed }.details["a"])
        repo.detailFails = false
        vm.retryDetail("a")
        val ready = assertIs<DetailState.Ready>(vm.state.first { it.details["a"] is DetailState.Ready }.details["a"])
        assertEquals("9f2c…b71e", ready.detail?.hash)
        assertEquals(false, vm.state.value.loading)

        vm.toggleLog("a")
        assertEquals(setOf("a"), vm.state.first { it.expandedLogs.isNotEmpty() }.expandedLogs)
        assertEquals("install", vm.logs("a").first().single().op)
        vm.toggleLog("a")
        assertEquals(emptySet<String>(), vm.state.first { it.expandedLogs.isEmpty() }.expandedLogs)
    }
}
