// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.framegen

import android.view.Surface
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.FrameGenProfile
import app.tatakai.next.model.FrameGenSettings
import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.test.TestScope
import kotlinx.coroutines.test.UnconfinedTestDispatcher
import kotlinx.coroutines.test.advanceUntilIdle
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

/**
 * 0.4.0 (worker B): the Tatakai parts of [FgController] on the JVM. `libtatakai_fg.so` never loads here, so every path that
 * would touch native code reports the device as unavailable, which is exactly the «No disponible» exit of the spec.
 */
@OptIn(ExperimentalCoroutinesApi::class)
class FgControllerTest {
  @get:Rule val tmp = TemporaryFolder()

  private class FakeCapabilities(
    override val abiOk: Boolean,
    override val vulkanLevel: Int = FgPolicy.VULKAN_1_3,
  ) : FgCapabilities {
    override fun currentHz(): Float = 120f

    override fun supportedHz(): List<Float> = listOf(60f, 120f)
  }

  /** Records what the controller asks mpv to do; no SurfaceView (the controller never starts FG without one). */
  private class FakePort : FgMpvPort {
    val calls = mutableListOf<String>()
    var audioDelay = 0.0

    override fun connect(surface: Surface, width: Int, height: Int) {
      calls += "connect"
    }

    override fun disconnect() {
      calls += "disconnect"
    }

    override fun setPropertyString(name: String, value: String) {
      calls += "$name=$value"
    }

    override fun getPropertyString(name: String): String? = null

    override fun getPropertyInt(name: String): Int? = null

    override fun getAudioDelaySec(): Double = audioDelay

    override fun setAudioDelaySec(sec: Double) {
      audioDelay = sec
    }

    override fun currentPath(): String = "/video.mkv"

    override fun outputSurface(): Surface? = null

    override fun outputSize(): Pair<Int, Int>? = null

    override fun setOutputVisible(visible: Boolean) = Unit

    override fun applyFrameRate(fps: Double, alwaysChange: Boolean) {
      calls += "rate=$fps"
    }

    override fun clearFrameRate() {
      calls += "clearRate"
    }
  }

  private fun TestScope.controller(abiOk: Boolean = true, store: FrameGenSettingsStore = InMemoryFrameGenSettingsStore()): FgController {
    val dispatcher = UnconfinedTestDispatcher(testScheduler)
    val installer = ShaderInstaller(
      filesDir = tmp.newFolder(),
      cacheDir = tmp.newFolder(),
      openUri = { null },
      available = { false },
      extract = { _, _ -> FgResult.NOT_INITIALIZED.code },
      probe = { false },
    )
    return FgController(
      prefs = FgPreferences(store),
      store = store,
      flag = FgActiveFlag(),
      shaderInstaller = installer,
      capabilities = FakeCapabilities(abiOk),
      scope = backgroundScope,
      mainDispatcher = dispatcher,
      pollDispatcher = dispatcher,
      ioDispatcher = dispatcher,
      nowMs = { testScheduler.currentTime },
    )
  }

  @Test
  fun `32-bit ABI is reported and the switch does not turn on`() = runTest {
    val c = controller(abiOk = false)
    c.setEnabled(true)
    assertFalse(c.state.value.enabled)
    assertEquals(FgUnavailableReason.ABI, c.state.value.unavailable)
  }

  @Test
  fun `without the native library FG is unavailable (LIBRARY) after a probe`() = runTest {
    val c = controller()
    c.refresh()
    advanceUntilIdle()
    val s = c.state.value
    assertEquals(FgUnavailableReason.LIBRARY, s.unavailable)
    assertTrue(s.eligibilityKnown)
    assertEquals(false, s.shadersImported)
    c.setEnabled(true)
    assertFalse(c.state.value.enabled)
  }

  @Test
  fun `ineligible wins over a Link suspension`() = runTest {
    val c = controller(abiOk = false)
    c.onRemote(active = true)
    assertEquals(FgUnavailableReason.ABI, c.state.value.unavailable)
  }

  @Test
  fun `bind and unbind track the bound session, a stale port is ignored`() = runTest {
    val c = controller()
    val first = FakePort()
    val second = FakePort()
    c.bind(first)
    assertTrue(c.state.value.bound)
    assertTrue(c.isBound(first))
    c.bind(second) // a new session replaces the first one
    assertFalse(c.isBound(first))
    c.unbind(first) // late release of the replaced session
    assertTrue(c.state.value.bound)
    c.onNativeDestroying()
    c.unbind(second)
    assertFalse(c.state.value.bound)
    // FG never started: mpv was never moved off the SurfaceView.
    assertTrue(second.calls.none { it == "connect" || it == "disconnect" })
  }

  @Test
  fun `surface callbacks are not handled while FG is off`() = runTest {
    val c = controller()
    c.bind(FakePort())
    assertFalse(c.onOutputSurfaceChanged(1920, 1080))
    assertFalse(c.onOutputSurfaceDestroyed())
  }

  @Test
  fun `choices are persisted in the store and published`() = runTest {
    val store = InMemoryFrameGenSettingsStore()
    val c = controller(store = store)
    c.setMultiplier(FrameGenMultiplier.X3)
    c.setProfile(FrameGenProfile.SAVER)
    c.setAudioOffsetMs(9_000)
    c.setMatchRefresh(false)
    c.setShowStats(true)
    val fg = store.settings.value
    assertEquals(3, fg.multiplier)
    assertEquals(FrameGenProfile.SAVER, fg.profile)
    assertEquals(500, fg.audioOffsetMs)
    assertFalse(fg.matchRefresh)
    assertTrue(fg.showStats)
    val s = c.state.value
    assertEquals(3, s.multiplier)
    assertFalse(s.auto)
    assertEquals(FrameGenProfile.SAVER, s.profile)
    assertEquals(fg, s.settings)
  }

  @Test
  fun `audio delay helpers are neutral while FG is off`() = runTest {
    val c = controller()
    assertEquals(0.25, c.userAudioDelaySec(0.25), 0.0)
    assertEquals(0.25, c.effectiveAudioDelaySec(0.25), 0.0)
    assertNull(c.frameRateFor(24.0))
  }

  @Test
  fun `8K detection`() {
    assertTrue(FgController.isEightK(7680 to 4320))
    assertTrue(FgController.isEightK(4320 to 7680))
    assertFalse(FgController.isEightK(3840 to 2160))
    assertFalse(FgController.isEightK(null))
  }

  @Test
  fun `multiplier choices map stored values`() {
    assertEquals(FrameGenMultiplier.AUTO, FrameGenMultiplier.of(0))
    assertEquals(FrameGenMultiplier.X5, FrameGenMultiplier.of(5))
    assertEquals(FrameGenMultiplier.AUTO, FrameGenMultiplier.of(7))
  }

  private class FakeSettings(initial: AppSettings = AppSettings()) : SettingsRepository {
    val state = MutableStateFlow(initial)
    override val settings: StateFlow<AppSettings> = state

    override suspend fun update(transform: (AppSettings) -> AppSettings) {
      state.value = transform(state.value)
    }
  }

  @Test
  fun `repository store writes through synchronously and follows outside changes`() = runTest {
    val repo = FakeSettings()
    val store = SettingsRepositoryFrameGenStore(repo, backgroundScope)
    store.update { it.copy(enabled = true, multiplier = 9) }
    // Visible at once (sanitized), persisted when the scope runs.
    assertEquals(FrameGenSettings(enabled = true), store.settings.value)
    runCurrent() // advanceUntilIdle stops when only backgroundScope work is left
    assertEquals(FrameGenSettings(enabled = true), repo.state.value.frameGen)
    repo.state.value = repo.state.value.copy(frameGen = FrameGenSettings(multiplier = 4))
    runCurrent()
    assertEquals(4, store.settings.value.multiplier)
  }
}
