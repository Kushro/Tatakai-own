// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(UnstableApi::class)

package app.tatakai.next.novel

import android.content.Context
import android.net.Uri
import android.os.Bundle
import android.speech.tts.TextToSpeech
import android.speech.tts.UtteranceProgressListener
import androidx.media3.common.AudioAttributes
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.ExoPlayer
import app.tatakai.next.model.Narrator
import app.tatakai.next.model.NovelChapterText
import app.tatakai.next.model.SecretKeys
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.TtsEngine
import app.tatakai.next.model.TtsSettings
import app.tatakai.next.model.TtsState
import app.tatakai.next.model.TtsVoice
import kotlinx.coroutines.CancellableContinuation
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import okhttp3.OkHttpClient
import java.io.File
import java.util.Locale
import java.util.concurrent.ConcurrentHashMap
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.6, worker s; checklist novel.tts.*): [Narrator] over the engine of
 * `AppSettings.tts`. SYSTEM = `android.speech.tts.TextToSpeech` (one utterance per paragraph chunk, `UtteranceProgressListener` advances
 * `currentParagraph`; `setSpeechRate`); OPENAI (`POST https://api.openai.com/v1/audio/speech`), ELEVENLABS
 * (`POST https://api.elevenlabs.io/v1/text-to-speech/<voice>`), OPENAI_COMPATIBLE (`<endpoint>/audio/speech`) synthesise each paragraph
 * to `cacheDir/tts/<sha256>.mp3` (prefetching the next one, [CloudTts]) and play it with an `ExoPlayer` owned by this class (speed =
 * playback speed). Keys come from [secrets] under `SecretKeys.ttsApiKey(engine.id)`; never logged.
 *
 * Pause stops the current utterance and remembers the paragraph; resume restarts that paragraph (TextToSpeech cannot pause mid-sentence).
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

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val cloud = CloudTts(client)
    private val queue = ParagraphQueue()
    private var job: Job? = null
    private var speedOverride: Float? = null

    // System engine.
    private var tts: TextToSpeech? = null
    private var ttsReady: CompletableDeferred<Boolean>? = null
    private val pendingUtterances = ConcurrentHashMap<String, CancellableContinuation<Unit>>()

    // Cloud engines.
    private var player: ExoPlayer? = null

    /** Last failure of a narration run (the reader shows it under the narration bar). */
    private val _lastError = MutableStateFlow<Throwable?>(null)
    val lastError: StateFlow<Throwable?> = _lastError.asStateFlow()

    override suspend fun speak(chapter: NovelChapterText, fromParagraph: Int) {
        stopEngines()
        job?.cancel()
        queue.load(chapter.paragraphs, fromParagraph)
        _lastError.value = null
        start()
    }

    override fun pause() {
        if (_state.value != TtsState.SPEAKING && _state.value != TtsState.PREPARING) return
        job?.cancel()
        job = null
        stopEngines()
        _state.value = TtsState.PAUSED
    }

    override fun resume() {
        if (_state.value != TtsState.PAUSED || queue.isEmpty) return
        start()
    }

    override fun stop() {
        job?.cancel()
        job = null
        stopEngines()
        queue.clear()
        _state.value = TtsState.IDLE
        _current.value = null
    }

    override fun setSpeed(speed: Float) {
        val s = if (speed.isFinite()) speed.coerceIn(0.5f, 2f) else 1f
        speedOverride = s
        tts?.setSpeechRate(s)
        scope.launch { player?.setPlaybackSpeed(s) }
    }

    override suspend fun voices(engine: TtsEngine): List<TtsVoice> {
        val current = settings().sanitized()
        return when (engine) {
            TtsEngine.SYSTEM -> {
                val engineTts = ensureSystem() ?: return emptyList()
                val app = Locale.getDefault().language
                val all = runCatching { engineTts.voices.orEmpty() }.getOrDefault(emptySet())
                    .map { v -> TtsVoice(v.name, "${v.locale.getDisplayName(Locale.getDefault())} · ${v.name}", v.locale.toLanguageTag()) }
                val mine = all.filter { it.lang?.startsWith(app) == true }
                (mine.ifEmpty { all }).sortedBy { it.label }
            }
            else -> cloud.voices(engine, current, apiKey(engine))
        }
    }

    override suspend fun preview(text: String): Result<Unit> = try {
        stop()
        val s = settings().sanitized()
        val speed = speedOverride ?: s.speed
        _state.value = TtsState.PREPARING
        when (s.engine) {
            TtsEngine.SYSTEM -> {
                val engineTts = ensureSystem() ?: throw TtsException(TtsFailure.ENGINE_UNAVAILABLE)
                applySystemVoice(engineTts, s)
                engineTts.setSpeechRate(speed)
                _state.value = TtsState.SPEAKING
                NovelText.speechChunks(text).forEachIndexed { i, chunk -> speakUtterance(engineTts, chunk, "preview-$i") }
            }
            else -> {
                val file = synth(s.engine, text, s)
                _state.value = TtsState.SPEAKING
                playFile(file, speed)
            }
        }
        _state.value = TtsState.IDLE
        Result.success(Unit)
    } catch (e: CancellationException) {
        _state.value = TtsState.IDLE
        throw e
    } catch (e: Exception) {
        _state.value = TtsState.IDLE
        Result.failure(e)
    }

    /** Releases the engines (Koin singleton: called by the reader when it is closed for good). */
    fun release() {
        stop()
        scope.launch {
            player?.release()
            player = null
        }
        tts?.shutdown()
        tts = null
        ttsReady = null
    }

    // ------------------------------------------------------------------------------------------------------------------- run loop

    private fun start() {
        job?.cancel()
        job = scope.launch {
            val s = settings().sanitized()
            val speed = speedOverride ?: s.speed
            _state.value = TtsState.PREPARING
            try {
                when (s.engine) {
                    TtsEngine.SYSTEM -> runSystem(s, speed)
                    else -> runCloud(s, speed)
                }
                // Reached the end of the chapter.
                queue.clear()
                _state.value = TtsState.IDLE
                _current.value = null
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                _lastError.value = e
                _state.value = TtsState.IDLE
                _current.value = null
            }
        }
    }

    private suspend fun runSystem(s: TtsSettings, speed: Float) {
        val engineTts = ensureSystem() ?: throw TtsException(TtsFailure.ENGINE_UNAVAILABLE)
        applySystemVoice(engineTts, s)
        engineTts.setSpeechRate(speed)
        while (true) {
            val (index, text) = queue.current() ?: return
            _current.value = index
            _state.value = TtsState.SPEAKING
            NovelText.speechChunks(text).forEachIndexed { i, chunk -> speakUtterance(engineTts, chunk, "p$index-$i") }
            queue.advance()
        }
    }

    private suspend fun runCloud(s: TtsSettings, speed: Float) {
        var prefetch: Deferred<File>? = null
        var prefetchIndex = -1
        while (true) {
            val (index, text) = queue.current() ?: return
            _current.value = index
            val file = if (prefetchIndex == index && prefetch != null) prefetch.await() else synth(s.engine, text, s)
            val next = queue.peekNext()
            if (next != null) {
                prefetchIndex = next.first
                prefetch = scope.async(Dispatchers.IO) { synth(s.engine, next.second, s) }
            } else {
                prefetch = null
                prefetchIndex = -1
            }
            _state.value = TtsState.SPEAKING
            playFile(file, speedOverride ?: speed)
            queue.advance()
        }
    }

    private suspend fun synth(engine: TtsEngine, text: String, s: TtsSettings): File {
        val dir = File(context.cacheDir, CACHE_DIR)
        val target = File(dir, CloudTts.cacheKey(engine, s, text) + ".mp3")
        return cloud.synthesize(engine, text, s, apiKey(engine), target)
    }

    private suspend fun apiKey(engine: TtsEngine): String? = runCatching { secrets.get(SecretKeys.ttsApiKey(engine.id)) }.getOrNull()

    // ------------------------------------------------------------------------------------------------------------------- system

    private suspend fun ensureSystem(): TextToSpeech? {
        tts?.let { existing -> if (ttsReady?.await() == true) return existing }
        val ready = CompletableDeferred<Boolean>()
        ttsReady = ready
        val created = withContext(Dispatchers.Main) {
            TextToSpeech(context.applicationContext) { status -> ready.complete(status == TextToSpeech.SUCCESS) }
        }
        created.setOnUtteranceProgressListener(listener)
        tts = created
        val ok = withTimeoutOrNull(INIT_TIMEOUT_MS) { ready.await() } ?: false
        if (!ok) {
            runCatching { created.shutdown() }
            tts = null
            ttsReady = null
            return null
        }
        return created
    }

    private fun applySystemVoice(engineTts: TextToSpeech, s: TtsSettings) {
        val wanted = s.voice
        val voices = runCatching { engineTts.voices.orEmpty() }.getOrDefault(emptySet())
        val voice = wanted?.let { name -> voices.firstOrNull { it.name == name } }
        if (voice != null) {
            engineTts.setVoice(voice)
        } else {
            runCatching { engineTts.setLanguage(Locale.getDefault()) }
        }
    }

    private suspend fun speakUtterance(engineTts: TextToSpeech, text: String, id: String): Unit = suspendCancellableCoroutine<Unit> { cont ->
        pendingUtterances[id] = cont
        cont.invokeOnCancellation {
            pendingUtterances.remove(id)
            runCatching { engineTts.stop() }
        }
        val result = engineTts.speak(text, TextToSpeech.QUEUE_FLUSH, Bundle(), id)
        if (result != TextToSpeech.SUCCESS) {
            pendingUtterances.remove(id)
            cont.resumeWithException(TtsException(TtsFailure.ENGINE_UNAVAILABLE))
        }
    }

    private val listener = object : UtteranceProgressListener() {
        override fun onStart(utteranceId: String?) {}

        override fun onDone(utteranceId: String?) {
            utteranceId?.let { pendingUtterances.remove(it) }?.resume(Unit)
        }

        @Deprecated("Deprecated in Java")
        override fun onError(utteranceId: String?) {
            utteranceId?.let { pendingUtterances.remove(it) }?.resumeWithException(TtsException(TtsFailure.ENGINE_UNAVAILABLE))
        }

        override fun onStop(utteranceId: String?, interrupted: Boolean) {
            // Stopped by pause / stop: the coroutine was cancelled already; nothing else is waiting.
            utteranceId?.let { pendingUtterances.remove(it) }?.cancel()
        }
    }

    // ------------------------------------------------------------------------------------------------------------------- cloud

    private suspend fun playFile(file: File, speed: Float): Unit = withContext(Dispatchers.Main) {
        val p = player ?: ExoPlayer.Builder(context.applicationContext)
            .setAudioAttributes(AudioAttributes.Builder().setUsage(C.USAGE_MEDIA).setContentType(C.AUDIO_CONTENT_TYPE_SPEECH).build(), true)
            .build()
            .also { player = it }
        suspendCancellableCoroutine<Unit> { cont ->
            val l = object : Player.Listener {
                override fun onPlaybackStateChanged(playbackState: Int) {
                    if (playbackState == Player.STATE_ENDED && cont.isActive) {
                        p.removeListener(this)
                        cont.resume(Unit)
                    }
                }

                override fun onPlayerError(error: PlaybackException) {
                    p.removeListener(this)
                    if (cont.isActive) cont.resumeWithException(TtsException(TtsFailure.NOT_AUDIO, cause = error))
                }
            }
            p.addListener(l)
            cont.invokeOnCancellation {
                scope.launch {
                    p.removeListener(l)
                    p.stop()
                }
            }
            p.setMediaItem(MediaItem.fromUri(Uri.fromFile(file)))
            p.setPlaybackSpeed(speed)
            p.prepare()
            p.play()
        }
    }

    private fun stopEngines() {
        pendingUtterances.keys.toList().forEach { id -> pendingUtterances.remove(id)?.cancel() }
        runCatching { tts?.stop() }
        scope.launch { player?.stop() }
    }

    companion object {
        const val CACHE_DIR: String = "tts"
        const val OPENAI_BASE_URL: String = "https://api.openai.com/v1"
        const val ELEVENLABS_BASE_URL: String = "https://api.elevenlabs.io/v1"
        private const val INIT_TIMEOUT_MS = 8_000L
    }
}

/**
 * The paragraphs of the chapter being narrated and the cursor (pure, JVM-tested): blank paragraphs are skipped, pause keeps the
 * cursor, the end of the chapter empties it.
 */
class ParagraphQueue {
    private var paragraphs: List<String> = emptyList()
    private var index: Int = 0

    val isEmpty: Boolean get() = current() == null

    fun load(paragraphs: List<String>, from: Int) {
        this.paragraphs = paragraphs
        index = from.coerceIn(0, paragraphs.size)
        skipBlank()
    }

    /** (index, text) of the paragraph to read now; null at the end. */
    fun current(): Pair<Int, String>? = paragraphs.getOrNull(index)?.let { index to it }

    fun peekNext(): Pair<Int, String>? {
        var i = index + 1
        while (i < paragraphs.size && paragraphs[i].isBlank()) i++
        return paragraphs.getOrNull(i)?.let { i to it }
    }

    fun advance() {
        index++
        skipBlank()
    }

    fun clear() {
        paragraphs = emptyList()
        index = 0
    }

    private fun skipBlank() {
        while (index < paragraphs.size && paragraphs[index].isBlank()) index++
    }
}
