// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.framegen

import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module

/**
 * Koin bindings of the phone frame generation (0.4.0, worker B). One app-wide [FgController]: the MPV session binds to it
 * (`MpvFrameGenBridge`), the player UI and Settings use it as [FrameGenController] (`koinInject<FrameGenController>()`).
 * Nothing native is loaded here: the library loads the first time FG is turned on or its state is probed ([FgNative]).
 */
val frameGenModule: Module = module {
  single { FgActiveFlag() }
  single<FrameGenSettingsStore> {
    val repository = getOrNull<SettingsRepository>()
    if (repository != null) {
      SettingsRepositoryFrameGenStore(repository, CoroutineScope(SupervisorJob() + Dispatchers.Default))
    } else {
      InMemoryFrameGenSettingsStore()
    }
  }
  single { FgPreferences(get()) }
  single { ShaderInstaller(androidContext()) }
  single<FgCapabilities> { AndroidFgCapabilities(androidContext()) }
  single {
    FgController(
      prefs = get(),
      store = get(),
      flag = get(),
      shaderInstaller = get(),
      capabilities = get(),
      // Main (not immediate): the controller posts its resumes and cycles; everything else it does is synchronous.
      scope = CoroutineScope(SupervisorJob() + Dispatchers.Main),
    )
  }
  single<FrameGenController> { get<FgController>() }
}
