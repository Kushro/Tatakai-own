// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.link

import app.tatakai.next.extensionserver.ExtensionServerConnection
import app.tatakai.next.extensionserver.ExtensionServerException
import app.tatakai.next.extensionserver.ExtensionServerRepository
import app.tatakai.next.extensionserver.PairingLink
import app.tatakai.next.extensionserver.PendingServerLink
import app.tatakai.next.extensionserver.ServerStatus
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
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
import kotlin.test.assertTrue

@OptIn(ExperimentalCoroutinesApi::class)
class ExtensionServersViewModelTest {
    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private val link = "tatakai://extensionserver/connect?server=http%3A%2F%2F192.168.1.20%3A8787&code=abc-123"

    private class FakeRepo : ExtensionServerRepository {
        val paired = mutableListOf<PairingLink>()
        var failWith: ExtensionServerException? = null
        override val servers = MutableStateFlow<List<ExtensionServerConnection>>(emptyList())
        override val loaded: StateFlow<Boolean> = MutableStateFlow(true)
        override val status = MutableStateFlow<Map<String, ServerStatus>>(emptyMap())

        override suspend fun connectWithLink(link: PairingLink): ExtensionServerConnection {
            failWith?.let { throw it }
            paired += link
            return ExtensionServerConnection("id", "srv", link.server, "Desk", "tok").also { servers.value = listOf(it) }
        }

        override suspend fun connectWithPassword(url: String, password: String): ExtensionServerConnection =
            throw ExtensionServerException(ExtensionServerException.INVALID_PASSWORD, 401)

        override suspend fun setEnabled(id: String, enabled: Boolean) {
            servers.value = servers.value.map { if (it.id == id) it.copy(enabled = enabled) else it }
        }

        override suspend fun remove(id: String) {
            servers.value = servers.value.filterNot { it.id == id }
        }

        override suspend fun refresh(id: String?) = Unit
    }

    @Test
    fun anOpenedLinkWaitsForConfirmation() = runTest {
        val repo = FakeRepo()
        val pending = PendingServerLink().also { it.offer(link) }
        val vm = ExtensionServersViewModel(repo, pending)
        assertEquals(PairingLink("http://192.168.1.20:8787", "abc-123"), vm.state.value.pendingLink)
        assertTrue(repo.paired.isEmpty(), "opening a link never pairs on its own")
        assertNull(pending.link.value)
        vm.confirmPending()
        assertEquals(1, repo.paired.size)
        assertEquals(1, vm.state.value.servers.size)
        assertNull(vm.state.value.pendingLink)
    }

    @Test
    fun scannedTextPairsAndErrorsAreMapped() = runTest {
        val repo = FakeRepo()
        val vm = ExtensionServersViewModel(repo, PendingServerLink())
        vm.connectWithText("tatakai://extensionserver/connect?server=http%3A%2F%2Fexample.org&code=x")
        assertIs<ExtServerError.InsecureHost>(vm.state.value.error)
        vm.connectWithText("hello")
        assertEquals(ExtServerError.InvalidLink, vm.state.value.error)
        repo.failWith = ExtensionServerException(ExtensionServerException.PAIRING_EXPIRED, 401)
        vm.connectWithText(link)
        assertEquals(ExtServerError.ExpiredLink, vm.state.value.error)
        vm.connectManual("http://192.168.1.20:8787", "nope")
        assertEquals(ExtServerError.WrongPassword, vm.state.value.error)
        vm.connectManual("http://192.168.1.20:8787/path", "x")
        assertEquals(ExtServerError.InvalidUrl, vm.state.value.error)
        vm.connectManual("http://8.8.8.8", "x")
        assertEquals(ExtServerError.InsecureHost("8.8.8.8"), vm.state.value.error)
    }
}
