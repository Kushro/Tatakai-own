package app.tatakai.next.feature.link

import app.cash.turbine.test
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.DiscoveredPc
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkProfile
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.LinkSource
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.PairedPc
import app.tatakai.next.model.RemoteSession
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.emptyFlow
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
class LinkViewModelTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private val pc = PairedPc("dev1", "Desktop", "192.168.1.9", 47321)
    private val caps = LinkCapabilities(
        serverName = "Desktop", serverVersion = "1", instanceId = "i", defaultProfile = "p-none",
        clientFrameGenerationAllowed = false,
        profiles = listOf(
            LinkProfile("p-none", "Original", false, false, 1920, 1080, "true"),
            LinkProfile("p-anime4k-1080", "Anime4K 1080p", true, false, 1920, 1080, "unknown"),
        ),
    )

    private class FakeLink(val pairResult: () -> Result<PairedPc>, val caps: () -> Result<LinkCapabilities>) : LinkRepository {
        override val state = MutableStateFlow<LinkState>(LinkState.Unpaired)
        var pairedPayload: String? = null
        var pairedName: String? = null
        var lastProfile: String? = "unset"
        var forgotten = false
        var session: () -> Result<RemoteSession> = { Result.success(RemoteSession("s1", 0, "https://pc/m.m3u8", null, 4, 60)) }
        override fun discover(): Flow<List<DiscoveredPc>> = emptyFlow()
        override suspend fun pair(payload: String, deviceName: String): Result<PairedPc> {
            pairedPayload = payload
            pairedName = deviceName
            return pairResult().onSuccess { state.value = LinkState.Paired(it, reachable = true) }
        }
        override suspend fun capabilities() = caps()
        override suspend fun startSession(source: LinkSource, profileId: String?): Result<RemoteSession> {
            lastProfile = profileId
            return session()
        }
        override suspend fun stop(sessionId: String) {}
        override suspend fun forget() {
            forgotten = true
            state.value = LinkState.Unpaired
        }
    }

    @Test
    fun `pairing passes the payload and device name then loads capabilities`() = runTest {
        val repo = FakeLink({ Result.success(pc) }, { Result.success(caps) })
        val vm = LinkViewModel(repo, "Pixel")

        vm.pair("  tatakai-link:v1?abc  ")

        assertEquals("tatakai-link:v1?abc", repo.pairedPayload)
        assertEquals("Pixel", repo.pairedName)
        val s = vm.state.value
        assertIs<LinkState.Paired>(s.link)
        assertIs<LoadState.Ok<LinkCapabilities>>(s.capabilities)
        assertNull(s.pairError)
    }

    @Test
    fun `pairing failure is reported and blank input is ignored`() = runTest {
        val repo = FakeLink({ Result.failure(AppException(AppError.Unknown("code expired"))) }, { Result.success(caps) })
        val vm = LinkViewModel(repo, "Pixel")

        vm.pair("   ")
        assertNull(repo.pairedPayload)

        vm.pair("tatakai-link:v1?old")
        assertEquals(AppError.Unknown("code expired"), vm.state.value.pairError)
        assertEquals(false, vm.state.value.pairing)
    }

    @Test
    fun `start session uses the selected profile and navigates with the remote session id`() = runTest {
        val repo = FakeLink({ Result.success(pc) }, { Result.success(caps) })
        val vm = LinkViewModel(repo, "Pixel")
        vm.pair("x")
        vm.selectProfile("p-anime4k-1080")

        vm.events.test {
            vm.startSession(" https://cdn.example/a.m3u8 ")
            val event = assertIs<LinkEvent.NavigateToPlayer>(awaitItem())
            assertEquals("s1", event.args.remoteSessionId)
            assertEquals("p-anime4k-1080", repo.lastProfile)
        }
    }

    @Test
    fun `session failure keeps the screen and exposes the error`() = runTest {
        val repo = FakeLink({ Result.success(pc) }, { Result.success(caps) })
        repo.session = { Result.failure(AppException(AppError.Http(503))) }
        val vm = LinkViewModel(repo, "Pixel")
        vm.pair("x")

        vm.startSession("https://cdn.example/a.m3u8")

        assertEquals(AppError.Http(503), vm.state.value.sessionError)
        assertEquals(false, vm.state.value.startingSession)
    }

    @Test
    fun `forgetting the pc clears capabilities`() = runTest {
        val repo = FakeLink({ Result.success(pc) }, { Result.success(caps) })
        val vm = LinkViewModel(repo, "Pixel")
        vm.pair("x")

        vm.forget()

        assertEquals(true, repo.forgotten)
        assertEquals(LinkState.Unpaired, vm.state.value.link)
        assertNull(vm.state.value.capabilities)
    }

    @Test
    fun `capability failure can be retried`() = runTest {
        var fail = true
        val repo = FakeLink({ Result.success(pc) }, { if (fail) Result.failure(AppException(AppError.Offline)) else Result.success(caps) })
        val vm = LinkViewModel(repo, "Pixel")
        vm.pair("x")
        assertIs<LoadState.Err>(vm.state.value.capabilities)

        fail = false
        vm.retryCapabilities()

        assertIs<LoadState.Ok<LinkCapabilities>>(vm.state.value.capabilities)
    }
}
