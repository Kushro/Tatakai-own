package app.tatakai.next.feature.extensions

import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionRepository
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.model.Media
import app.tatakai.next.model.StreamSource
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
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
}
