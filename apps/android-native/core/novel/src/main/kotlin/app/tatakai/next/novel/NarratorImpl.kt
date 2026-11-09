// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import android.content.Context
import app.tatakai.next.model.Narrator
import app.tatakai.next.model.NovelChapterText
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.TtsEngine
import app.tatakai.next.model.TtsSettings
import app.tatakai.next.model.TtsState
import app.tatakai.next.model.TtsVoice
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import okhttp3.OkHttpClient

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.6, worker s; checklist novel.tts.*): [Narrator] over the engine of
 * `AppSettings.tts`. SYSTEM = `android.speech.tts.TextToSpeech` (one utterance per paragraph, `UtteranceProgressListener` advances
 * `currentParagraph`; `setSpeechRate`); OPENAI (`POST https://api.openai.com/v1/audio/speech`, `model`, `voice`, `speed`, mp3),
 * ELEVENLABS (`POST https://api.elevenlabs.io/v1/text-to-speech/<voice>`), OPENAI_COMPATIBLE (`<endpoint>/audio/speech`) synthesise each
 * paragraph to `cacheDir/tts/<sha256>.mp3` (prefetching the next one) and play it with an `ExoPlayer` owned by this class. Keys come from
 * [secrets] under `SecretKeys.ttsApiKey(engine.id)`; never logged. `voices(SYSTEM)` = `TextToSpeech.voices` filtered by the app language.
 *
 * Stub: idle forever; `speak` / `preview` fail with a clear error.
 */
class NarratorImpl(
    private val context: Context,
    private val client: OkHttpClient,
    private val secrets: SecretStore,
    private val settings: () -> TtsSettings,
) : Narrator {
    private val _state = MutableStateFlow(TtsState.IDLE)
    override val state: StateFlow<TtsState> = _state.asStateFlow()
    private val _current = MutableStateFlow<Int?>(null)
    override val currentParagraph: StateFlow<Int?> = _current.asStateFlow()

    override suspend fun speak(chapter: NovelChapterText, fromParagraph: Int) {
        throw NotImplementedError("wave B worker s: NarratorImpl.speak")
    }

    override fun pause() {}

    override fun resume() {}

    override fun stop() {
        _state.value = TtsState.IDLE
        _current.value = null
    }

    override fun setSpeed(speed: Float) {}

    override suspend fun voices(engine: TtsEngine): List<TtsVoice> = emptyList()

    override suspend fun preview(text: String): Result<Unit> = Result.failure(NotImplementedError("wave B worker s: NarratorImpl.preview"))

    companion object {
        const val CACHE_DIR: String = "tts"
        const val OPENAI_BASE_URL: String = "https://api.openai.com/v1"
        const val ELEVENLABS_BASE_URL: String = "https://api.elevenlabs.io/v1"
    }
}
