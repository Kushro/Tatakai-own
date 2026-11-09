// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.6): light novels (home, details, chapters by extension,
// reader with typography, narration / TTS, readlist, chapter downloads). Desktop counterpart: tk-core/novel/** + tk-app/src/tts.rs
// (worker m). Signatures FINAL; bodies: worker s (:core:novel `NovelRepositoryImpl` + `Narrator`, bound in `novelModule`; UI in
// :feature:novel). Titles are AniList MANGA with `format == "NOVEL"` (see Manga.kt header). Novel extension sources are custom sources of
// kind `novel` (`ExtensionRepository.customSources("novel")`). KittenTTS (desktop m) has no Android build: checklist `novel.tts.kitten`
// stays desktop-only.

data class NovelSourceSummary(
    val extensionId: String,
    val extensionName: String,
    val sourceId: String,
    val itemId: String?,
    val languages: List<String>,
    /** Filled by "Comprobar todas" (`NovelRepository.checkSources`); null = not checked yet. */
    val chapterCount: Int?,
    val translator: String? = null,
)

@Serializable
data class NovelChapter(
    val id: String,
    val extensionId: String,
    val sourceId: String,
    val itemId: String,
    val number: Double? = null,
    val title: String? = null,
    val lang: String? = null,
    val translator: String? = null,
    val releasedAtMs: Long? = null,
    val downloaded: Boolean = false,
)

/** The text of a chapter (`custom/read` of a `novel` source), one string per paragraph; HTML already stripped. */
@Serializable
data class NovelChapterText(val chapterId: String, val title: String?, val paragraphs: List<String>)

/** Where the user left a novel: [paragraph] index + [offset] 0..1 inside the chapter (classic "Continue reading"). */
@Serializable
data class NovelProgress(
    val mediaId: String,
    val title: String,
    val poster: String? = null,
    val chapterId: String,
    val chapterNumber: Double? = null,
    val extensionId: String,
    val sourceId: String,
    val itemId: String,
    val paragraph: Int,
    val offset: Float,
    val updatedAtMs: Long,
)

/** Settings › Lector de novela (`AppSettings.novelReader`), classic NovelReaderPanel. */
@Serializable
data class NovelReaderSettings(
    /** `sans` | `serif` | `georgia` | `lora` | `dyslexic` | `system` | `custom` ([customFontPath]). */
    val font: String = "serif",
    /** `filesDir/fonts/novel/<file>` of a TTF / OTF the user imported (`novel.reader.custom-font`); null = none. */
    val customFontPath: String? = null,
    val sizeSp: Int = 18,
    /** Line height multiplier, 1.0..2.5. */
    val lineHeight: Float = 1.6f,
    val paragraphSpacingDp: Int = 12,
    /** 0 = no limit. */
    val maxWidthDp: Int = 720,
    /** `theme` | `sepia` | `black` | `white` | `gray`. */
    val background: String = "theme",
    val textAlignJustify: Boolean = false,
    val continuousScroll: Boolean = true,
    val showProgressBar: Boolean = true,
    val fullscreenOnOpen: Boolean = true,
    val keepScreenOn: Boolean = true,
) {
    fun sanitized(): NovelReaderSettings = copy(
        font = font.takeIf { it in FONTS } ?: "serif",
        sizeSp = sizeSp.coerceIn(10, 40),
        lineHeight = if (lineHeight.isFinite()) lineHeight.coerceIn(1f, 2.5f) else 1.6f,
        paragraphSpacingDp = paragraphSpacingDp.coerceIn(0, 48),
        maxWidthDp = maxWidthDp.coerceIn(0, 4000),
        background = background.takeIf { it in BACKGROUNDS } ?: "theme",
    )

    companion object {
        val FONTS: Set<String> = setOf("sans", "serif", "georgia", "lora", "dyslexic", "system", "custom")
        val BACKGROUNDS: Set<String> = setOf("theme", "sepia", "black", "white", "gray")
    }
}

/** Narration engines (classic TtsIntegrationSettings). Keys live in `SecretStore` under `SecretKeys.ttsApiKey(engine.id)`. */
@Serializable
enum class TtsEngine(val id: String) {
    @SerialName("system") SYSTEM("system"),
    @SerialName("openai") OPENAI("openai"),
    @SerialName("elevenlabs") ELEVENLABS("elevenlabs"),
    @SerialName("openai_compatible") OPENAI_COMPATIBLE("openai_compatible"),
}

/** Settings › Lector de novela › Narrador (`AppSettings.tts`). No API key here (secrets never live in settings). */
@Serializable
data class TtsSettings(
    val engine: TtsEngine = TtsEngine.SYSTEM,
    /** Engine voice id (system: `Voice.name`; OpenAI: `alloy`…; ElevenLabs: voice id); null = engine default. */
    val voice: String? = null,
    /** 0.5..2.0. */
    val speed: Float = 1f,
    /** OPENAI_COMPATIBLE base URL (`http://127.0.0.1:8880/v1`); ignored by the others. */
    val endpoint: String = "",
    /** OpenAI / compatible model (`tts-1`); ignored by SYSTEM / ELEVENLABS. */
    val model: String = "",
    /** Highlight the paragraph being read. */
    val highlight: Boolean = true,
) {
    fun sanitized(): TtsSettings = copy(
        speed = if (speed.isFinite()) speed.coerceIn(0.5f, 2f) else 1f,
        endpoint = endpoint.trim(),
        model = model.trim(),
        voice = voice?.trim()?.takeIf { it.isNotEmpty() },
    )
}

enum class TtsState { IDLE, PREPARING, SPEAKING, PAUSED }

/** One voice an engine offers ("Probar voz" plays [previewText] of the settings sheet). */
data class TtsVoice(val id: String, val label: String, val lang: String?)

/**
 * "Leer en voz alta": narrates a chapter paragraph by paragraph with the engine of `AppSettings.tts`. System = `android.speech.tts.TextToSpeech`;
 * cloud engines synthesise paragraph audio (cached in `cacheDir/tts/`) and play it with Media3. Implemented by worker s (`NarratorImpl`).
 */
interface Narrator {
    val state: StateFlow<TtsState>

    /** Index of the paragraph being read; null when idle. */
    val currentParagraph: StateFlow<Int?>

    /** Starts (or restarts) at [fromParagraph]. */
    suspend fun speak(chapter: NovelChapterText, fromParagraph: Int = 0)

    fun pause()

    fun resume()

    fun stop()

    fun setSpeed(speed: Float)

    suspend fun voices(engine: TtsEngine): List<TtsVoice>

    /** "Probar voz": one sentence with the current settings. */
    suspend fun preview(text: String): Result<Unit>
}

/** `Route.NovelReader(args)`. Same conventions as [MangaReaderArgs]. */
@Serializable
data class NovelReaderArgs(
    val mediaId: String,
    val title: String,
    val poster: String? = null,
    val extensionId: String,
    val sourceId: String,
    val itemId: String,
    val chapterId: String,
    val chapterNumber: Double? = null,
    val startParagraph: Int = 0,
    val offline: Boolean = false,
    val anilistId: Int? = null,
)

data class NovelHome(
    val hero: LoadState<List<Media>> = LoadState.Loading,
    val trending: LoadState<List<Media>> = LoadState.Loading,
    val popular: LoadState<List<Media>> = LoadState.Loading,
    val genres: List<String> = emptyList(),
    val lastRead: List<NovelProgress> = emptyList(),
    val followedUpdates: List<Pair<ReadEntry, NovelChapter>> = emptyList(),
)

/** Implemented by worker s (`NovelRepositoryImpl`, :core:novel). */
interface NovelRepository {
    fun home(): Flow<NovelHome>

    suspend fun refreshHome()

    /** `searchAdvanced(MANGA, query.copy(formats = ["NOVEL"]))`. */
    suspend fun search(query: MediaSearchQuery, page: Int = 1): FeedPage

    suspend fun sources(ref: MediaRef, title: String): List<NovelSourceSummary>

    /** "Comprobar todas": asks every source for its chapter count; returns the summaries with [NovelSourceSummary.chapterCount] filled. */
    suspend fun checkSources(ref: MediaRef, title: String): List<NovelSourceSummary>

    suspend fun chapters(source: NovelSourceSummary, refresh: Boolean = false): List<NovelChapter>

    suspend fun text(chapter: NovelChapter): NovelChapterText

    fun lastRead(limit: Int = 20): Flow<List<NovelProgress>>

    fun progress(mediaId: String): Flow<NovelProgress?>

    suspend fun saveProgress(progress: NovelProgress)

    fun readlist(status: ListStatus? = null): Flow<List<ReadEntry>>

    fun readEntry(ref: MediaRef): Flow<ReadEntry?>

    suspend fun setReadStatus(media: Media, status: ListStatus?): Result<Unit>

    suspend fun setReadProgress(ref: MediaRef, chapters: Int): Result<Unit>

    /** Stores the chapter text locally (SQLite / files of :core:novel); listed by the Downloads page through `ChapterDownloadSource`. */
    suspend fun downloadChapters(media: Media, chapters: List<NovelChapter>): Result<Int>

    fun downloadedChapters(mediaId: String): Flow<List<NovelChapter>>

    suspend fun removeDownloadedChapter(chapterId: String)

    suspend fun offlineText(chapterId: String): NovelChapterText?
}
