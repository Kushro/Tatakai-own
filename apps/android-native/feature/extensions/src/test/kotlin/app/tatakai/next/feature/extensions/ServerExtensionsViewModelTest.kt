// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import app.tatakai.next.extensionserver.ExtensionServerConnection
import app.tatakai.next.extensionserver.ExtensionServerException
import app.tatakai.next.extensionserver.ExtensionServerRepository
import app.tatakai.next.extensionserver.PairingLink
import app.tatakai.next.extensionserver.ServerExtension
import app.tatakai.next.extensionserver.ServerStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import java.io.IOException
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class ServerExtensionsViewModelTest {
    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private class FakeRepo : ExtensionServerRepository {
        val calls = mutableListOf<String>()
        val lists = mutableMapOf<String, List<ServerExtension>>()
        val failures = mutableMapOf<String, Exception>()
        override val servers = MutableStateFlow<List<ExtensionServerConnection>>(emptyList())
        override val loaded = MutableStateFlow(false)
        override val status = MutableStateFlow<Map<String, ServerStatus>>(emptyMap())

        override suspend fun connectWithLink(link: PairingLink): ExtensionServerConnection = error("unused")
        override suspend fun connectWithPassword(url: String, password: String): ExtensionServerConnection = error("unused")
        override suspend fun setEnabled(id: String, enabled: Boolean) {
            servers.value = servers.value.map { if (it.id == id) it.copy(enabled = enabled) else it }
        }
        override suspend fun remove(id: String) = Unit
        override suspend fun refresh(id: String?) = Unit

        override suspend fun extensions(id: String): List<ServerExtension> {
            calls += id
            failures[id]?.let { throw it }
            return lists[id].orEmpty()
        }
    }

    private fun conn(id: String, enabled: Boolean = true) = ExtensionServerConnection(id, "srv-$id", "http://192.168.1.$id:8787", "PC $id", "tok", enabled)

    @Test
    fun listsTheExtensionsOfEveryEnabledServer() = runTest {
        val repo = FakeRepo()
        repo.lists["1"] = listOf(ServerExtension("b", "Beta", enabled = false), ServerExtension("a", "alpha"))
        val vm = ServerExtensionsViewModel(repo)
        assertFalse(vm.state.value.loaded)
        repo.servers.value = listOf(conn("1"), conn("2", enabled = false))
        repo.loaded.value = true

        val blocks = vm.state.value.servers
        assertEquals(listOf("1", "2"), blocks.map { it.id })
        val ok = assertIs<ServerExtensionsLoad.Ok>(blocks[0].load)
        assertEquals(listOf("a", "b"), ok.extensions.map { it.id }, "enabled first, then by name")
        assertEquals(ServerExtensionsLoad.Disabled, blocks[1].load)
        assertEquals(listOf("1"), repo.calls, "a disabled server is never asked")
    }

    @Test
    fun errorsAreMappedAndRetryReadsAgain() = runTest {
        val repo = FakeRepo()
        repo.failures["1"] = ExtensionServerException(ExtensionServerException.UNREACHABLE)
        repo.servers.value = listOf(conn("1"))
        repo.loaded.value = true
        val vm = ServerExtensionsViewModel(repo)
        assertEquals(ServerExtensionsLoad.Err(ServerExtError.UNREACHABLE), vm.state.value.servers.single().load)

        repo.failures.clear()
        repo.lists["1"] = listOf(ServerExtension("x", "X"))
        vm.refresh("1")
        val ok = assertIs<ServerExtensionsLoad.Ok>(vm.state.value.servers.single().load)
        assertEquals(listOf("x"), ok.extensions.map { it.id })
        assertEquals(2, repo.calls.size)
    }

    @Test
    fun turningAServerBackOnReadsItAgain() = runTest {
        val repo = FakeRepo()
        repo.servers.value = listOf(conn("1"))
        repo.loaded.value = true
        val vm = ServerExtensionsViewModel(repo)
        repo.setEnabled("1", false)
        assertEquals(ServerExtensionsLoad.Disabled, vm.state.value.servers.single().load)
        repo.setEnabled("1", true)
        assertIs<ServerExtensionsLoad.Ok>(vm.state.value.servers.single().load)
        assertEquals(listOf("1", "1"), repo.calls)
    }

    @Test
    fun noClientHidesTheSection() {
        val vm = ServerExtensionsViewModel(null)
        assertFalse(vm.state.value.available)
        assertTrue(vm.state.value.servers.isEmpty())
    }

    @Test
    fun errorMapping() {
        assertEquals(ServerExtError.UNAUTHORIZED, serverExtErrorOf(ExtensionServerException("x", 401)))
        assertEquals(ServerExtError.UNAUTHORIZED, serverExtErrorOf(ExtensionServerException(ExtensionServerException.PAIRING_REQUIRED, 403)))
        assertEquals(ServerExtError.INCOMPATIBLE, serverExtErrorOf(ExtensionServerException(ExtensionServerException.BAD_RESPONSE)))
        assertEquals(ServerExtError.UNREACHABLE, serverExtErrorOf(IOException("boom")))
        assertEquals(ServerExtError.OTHER, serverExtErrorOf(ExtensionServerException("internal_error", 500)))
    }

    @Test
    fun kindsAreSplitAndDeduplicated() {
        assertEquals(listOf("watch", "read"), serverExtensionKinds(" watch,read, watch ,,"))
        assertTrue(serverExtensionKinds("").isEmpty())
    }
}
