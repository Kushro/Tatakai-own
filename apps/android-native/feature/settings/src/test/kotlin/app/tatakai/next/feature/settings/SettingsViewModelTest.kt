package app.tatakai.next.feature.settings

import app.tatakai.next.model.AccountSuspendedException
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.AuthRepository
import app.tatakai.next.model.ContrastPref
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.UserSession
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
import kotlin.test.assertNull

@OptIn(ExperimentalCoroutinesApi::class)
class SettingsViewModelTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private class FakeSettings : SettingsRepository {
        override val settings = MutableStateFlow(AppSettings())
        override suspend fun update(transform: (AppSettings) -> AppSettings) {
            settings.value = transform(settings.value)
        }
    }

    private class FakeAuth(var signInResult: Result<Unit> = Result.success(Unit)) : AuthRepository {
        override val session = MutableStateFlow<UserSession?>(null)
        override suspend fun signIn(email: String, password: String): Result<Unit> {
            signInResult.onSuccess { session.value = UserSession("u1", email) }
            return signInResult
        }
        override suspend fun signOut() {
            session.value = null
        }
    }

    @Test
    fun `appearance changes are written through the settings repository`() = runTest {
        val repo = FakeSettings()
        val vm = SettingsViewModel(repo, FakeAuth())

        vm.setPalette("sakura")
        vm.setAmoled(true)
        vm.setDynamicColor(false)
        vm.setContrast(ContrastPref.HIGH)

        val s = repo.settings.value
        assertEquals("sakura", s.paletteId)
        assertEquals(true, s.amoled)
        assertEquals(false, s.dynamicColor)
        assertEquals(ContrastPref.HIGH, s.contrast)
        assertEquals("sakura", vm.state.first { it.settings.paletteId == "sakura" }.settings.paletteId)
    }

    @Test
    fun `provider priority moves and clamps`() = runTest {
        val repo = FakeSettings()
        val vm = SettingsViewModel(repo, FakeAuth())

        vm.moveProvider("kitsu", -1)
        assertEquals(listOf("anilist", "kitsu", "anizip"), repo.settings.value.providerPriority)
        vm.moveProvider("anilist", -1)
        assertEquals(listOf("anilist", "kitsu", "anizip"), repo.settings.value.providerPriority)
        assertEquals(listOf("a", "b"), listOf("a", "b").moved("zzz", 1))
    }

    @Test
    fun `sign in success exposes the session and sign out clears it`() = runTest {
        val auth = FakeAuth()
        val vm = SettingsViewModel(FakeSettings(), auth)

        vm.signIn(" me@x.test ", "pw")
        assertEquals("me@x.test", vm.state.first { it.session != null }.session?.email)
        assertNull(vm.state.value.signInError)

        vm.signOut()
        assertNull(vm.state.first { it.session == null }.session)
    }

    @Test
    fun `sign in failures distinguish suspended accounts`() = runTest {
        val auth = FakeAuth(Result.failure(IllegalStateException("bad")))
        val vm = SettingsViewModel(FakeSettings(), auth)

        vm.signIn("a@b.c", "x")
        assertEquals(SignInError.INVALID, vm.state.first { it.signInError != null }.signInError)

        auth.signInResult = Result.failure(AccountSuspendedException())
        vm.signIn("a@b.c", "x")
        assertEquals(SignInError.SUSPENDED, vm.state.first { it.signInError == SignInError.SUSPENDED }.signInError)
    }
}
