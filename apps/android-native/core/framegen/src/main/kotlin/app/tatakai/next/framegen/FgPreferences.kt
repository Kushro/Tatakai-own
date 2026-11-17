/*
 * SPDX-License-Identifier: AGPL-3.0-or-later
 *
 * This program is free software: you can redistribute it and/or modify
 * it under the terms of the GNU Affero General Public License as published
 * by the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 */

package app.tatakai.next.framegen

import app.tatakai.next.model.FrameGenSettings
import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch

/**
 * Where the frame generation settings live. [settings] is read synchronously and [update] is visible in [settings] at once
 * (the ported controller writes a preference and reads it back in the same call, as Enigmata's PreferenceStore allowed).
 */
interface FrameGenSettingsStore {
  val settings: StateFlow<FrameGenSettings>

  fun update(transform: (FrameGenSettings) -> FrameGenSettings)
}

/** In-memory store (tests, and the fallback when no [SettingsRepository] is bound). */
class InMemoryFrameGenSettingsStore(initial: FrameGenSettings = FrameGenSettings()) : FrameGenSettingsStore {
  private val state = MutableStateFlow(initial.sanitized())
  override val settings: StateFlow<FrameGenSettings> = state.asStateFlow()

  override fun update(transform: (FrameGenSettings) -> FrameGenSettings) {
    state.value = transform(state.value).sanitized()
  }
}

/**
 * [FrameGenSettingsStore] over `AppSettings.frameGen` of the app [SettingsRepository] (DataStore, `frame_gen_json`).
 *
 * A local mirror makes writes visible synchronously; the DataStore write runs on [scope]. Values coming from the repository
 * (another screen, a restore) replace the mirror only when the repository value itself changed, so a write that has not been
 * persisted yet is never undone by the previous stored value.
 */
class SettingsRepositoryFrameGenStore(
  private val repository: SettingsRepository,
  private val scope: CoroutineScope,
) : FrameGenSettingsStore {
  private var lastFromRepository = repository.settings.value.frameGen.sanitized()
  private val mirror = MutableStateFlow(lastFromRepository)
  override val settings: StateFlow<FrameGenSettings> = mirror.asStateFlow()

  init {
    scope.launch {
      repository.settings.map { it.frameGen.sanitized() }.distinctUntilChanged().collect {
        // Only a real change of the stored value (the first emission repeats the construction value).
        if (it != lastFromRepository) {
          lastFromRepository = it
          mirror.value = it
        }
      }
    }
  }

  override fun update(transform: (FrameGenSettings) -> FrameGenSettings) {
    val next = transform(mirror.value).sanitized()
    if (next == mirror.value) return
    mirror.value = next
    scope.launch { repository.update { it.copy(frameGen = next) } }
  }
}

/**
 * One frame generation preference: synchronous [get] / [set] and a [changes] flow (the shape of Enigmata's `Preference<T>`, so
 * the ported [FgController] keeps its code).
 */
class FgPref<T> internal constructor(
  private val store: FrameGenSettingsStore,
  private val read: (FrameGenSettings) -> T,
  private val write: (FrameGenSettings, T) -> FrameGenSettings,
) {
  fun get(): T = read(store.settings.value)

  fun set(value: T) = store.update { write(it, value) }

  fun changes(): Flow<T> = store.settings.map(read).distinctUntilChanged()
}

/**
 * R13 frame generation preferences (Enigmata `preferences/FgPreferences.kt`), all global (never per file), mapped 1:1 onto
 * [FrameGenSettings] (same defaults). See [FrameGenSettings] for each value.
 */
class FgPreferences(store: FrameGenSettingsStore) {
  /** User switch; off by default. */
  val enabled = FgPref(store, { it.enabled }) { s, v -> s.copy(enabled = v) }

  /** [MULTIPLIER_AUTO] (0) = Auto, otherwise one of [MULTIPLIER_RANGE] (2..5). */
  val multiplier = FgPref(store, { it.multiplier }) { s, v -> s.copy(multiplier = v) }

  val profile = FgPref(store, { it.profile }) { s, v -> s.copy(profile = v) }

  // Values of FgProfile.CUSTOM.
  val customPerformance = FgPref(store, { it.customPerformance }) { s, v -> s.copy(customPerformance = v) }
  val customFlowScale = FgPref(store, { it.customFlowScale }) { s, v -> s.copy(customFlowScale = v) }
  val customInputScale = FgPref(store, { it.customInputScale }) { s, v -> s.copy(customInputScale = v) }

  /** «Change the refresh rate for FG»; on by default. */
  val matchRefreshForFg = FgPref(store, { it.matchRefresh }) { s, v -> s.copy(matchRefresh = v) }

  /** Manual audio offset in ms, within [AUDIO_OFFSET_RANGE_MS]; 0 by default. */
  val audioOffsetMs = FgPref(store, { it.audioOffsetMs }) { s, v -> s.copy(audioOffsetMs = v) }

  /** «Allow FG on untested GPUs (experimental)»: Vulkan 1.1 / 1.2 become eligible, always marked experimental. */
  val allowUntestedVulkan = FgPref(store, { it.allowUntestedVulkan }) { s, v -> s.copy(allowUntestedVulkan = v) }

  /** «Show FG stats»: overlay in the player and a logcat summary line about once per second. */
  val showStatsOverlay = FgPref(store, { it.showStats }) { s, v -> s.copy(showStats = v) }

  /** Floor of the video-fit input size (`FgPolicy.inputSize(..., floor)`); readers pass it through `snapVideoFitFloor`. */
  val videoFitFloor = FgPref(store, { it.videoFitFloor }) { s, v -> s.copy(videoFitFloor = v) }

  /** What FG gives up when the upscaler and the GPU budget compete ([FgPriority]). */
  val priority = FgPref(store, { it.priority }) { s, v -> s.copy(priority = v) }

  /** «FG at screen resolution»: input = output × profile scale even if the GPU budget is exceeded. */
  val screenResolution = FgPref(store, { it.screenResolution }) { s, v -> s.copy(screenResolution = v) }

  companion object {
    const val MULTIPLIER_AUTO = FrameGenSettings.MULTIPLIER_AUTO
    val MULTIPLIER_RANGE = FrameGenSettings.MULTIPLIER_RANGE
    val AUDIO_OFFSET_RANGE_MS = FrameGenSettings.AUDIO_OFFSET_RANGE_MS
  }
}
