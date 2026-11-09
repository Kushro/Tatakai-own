// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.PlayerEngine
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.resetMain
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.test.setMain
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** parity wave A closure (worker q, account.setup-wizard): engine / source preferences and permissions steps of the first-run wizard. */
@OptIn(ExperimentalCoroutinesApi::class)
class OnboardingTest {

    @BeforeTest fun setUp() = Dispatchers.setMain(UnconfinedTestDispatcher())

    @AfterTest fun tearDown() = Dispatchers.resetMain()

    private class FakeSettings : SettingsRepository {
        override val settings = MutableStateFlow(AppSettings())
        override suspend fun update(transform: (AppSettings) -> AppSettings) {
            settings.value = transform(settings.value)
        }
    }

    @Test
    fun `steps cover language, playback, lists, pc and permissions in order`() {
        assertEquals(
            listOf(
                OnboardingStep.WELCOME, OnboardingStep.APPEARANCE, OnboardingStep.PLAYBACK, OnboardingStep.LISTS,
                OnboardingStep.LINK, OnboardingStep.PERMISSIONS, OnboardingStep.DONE,
            ),
            onboardingSteps(notificationsNeedPermission = true),
        )
        val old = onboardingSteps(notificationsNeedPermission = false)
        assertFalse(OnboardingStep.PERMISSIONS in old)
        assertTrue(OnboardingStep.PLAYBACK in old)
        assertEquals(OnboardingStep.DONE, old.last())
    }

    @Test
    fun `playback step writes the engine and source preferences`() = runTest {
        val repo = FakeSettings()
        val vm = OnboardingViewModel(repo)
        vm.setPlayerEngine(PlayerEngine.MPV)
        vm.setSourceMode(SourceMode.MANUAL)
        vm.setPreferredKind(SourceKind.TORRENT)
        vm.setMinQuality(Quality.P1080)
        vm.setSubLang("en")
        vm.setDubLang("es")
        val s = repo.settings.value
        assertEquals(PlayerEngine.MPV, s.playerEngine)
        assertEquals(SourceMode.MANUAL, s.sourcePrefs.mode)
        assertEquals(SourceKind.TORRENT, s.sourcePrefs.prefer)
        assertEquals(Quality.P1080, s.sourcePrefs.minQuality)
        assertEquals("en", s.sourcePrefs.subLang)
        assertEquals("es", s.sourcePrefs.dubLang)
        assertFalse(s.onboardingDone)

        var done = false
        vm.finish { done = true }
        assertTrue(done)
        assertTrue(repo.settings.value.onboardingDone)
    }
}
