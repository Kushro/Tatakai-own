// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import app.tatakai.next.manga.ReaderJson
import app.tatakai.next.manga.list
import app.tatakai.next.manga.str
import app.tatakai.next.manga.sub
import app.tatakai.next.model.TtsEngine
import app.tatakai.next.model.TtsSettings
import app.tatakai.next.model.TtsVoice
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.File
import java.io.IOException
import java.security.MessageDigest

// Cloud narration engines (checklist novel.tts.openai / novel.tts.elevenlabs / novel.tts.openai-compatible), port of the classic
// src/lib/novel/ttsIntegrations.ts. Audio is always synthesised at speed 1 and the speed is applied by the player, so changing the speed
// is instant and the cache stays valid. Keys come from SecretStore (SecretKeys.ttsApiKey) and are never logged.

/** Why a narration request could not run; the screens map it to a localized message. */
enum class TtsFailure { MISSING_KEY, MISSING_VOICE, MISSING_MODEL, BAD_ENDPOINT, HTTP, NOT_AUDIO, EMPTY_AUDIO, NETWORK, ENGINE_UNAVAILABLE }

class TtsException(val reason: TtsFailure, val httpCode: Int? = null, cause: Throwable? = null) :
    Exception("tts: $reason${httpCode?.let { " ($it)" } ?: ""}", cause)

/** One HTTP request to a speech API (built by [CloudTts.request], pure). */
data class TtsRequestSpec(val url: String, val headers: Map<String, String>, val body: String)

class CloudTts(
    private val client: OkHttpClient,
    private val openAiBase: String = NarratorImpl.OPENAI_BASE_URL,
    private val elevenLabsBase: String = NarratorImpl.ELEVENLABS_BASE_URL,
) {
    /** Synthesises [text] into [target] (mp3) unless it is cached already; returns [target]. */
    suspend fun synthesize(engine: TtsEngine, text: String, settings: TtsSettings, apiKey: String?, target: File): File = withContext(Dispatchers.IO) {
        if (target.isFile && target.length() > 0) return@withContext target
        val spec = request(engine, text, settings, apiKey, openAiBase, elevenLabsBase)
        val body = spec.body.toRequestBody(JSON)
        val req = Request.Builder().url(spec.url).apply { spec.headers.forEach { (k, v) -> header(k, v) } }.post(body).build()
        try {
            client.newCall(req).execute().use { response ->
                if (!response.isSuccessful) throw TtsException(TtsFailure.HTTP, response.code)
                val type = response.header("Content-Type").orEmpty()
                if (type.contains("json") || type.startsWith("text/")) throw TtsException(TtsFailure.NOT_AUDIO)
                target.parentFile?.mkdirs()
                val tmp = File(target.parentFile, target.name + ".part")
                tmp.outputStream().use { out -> response.body.byteStream().use { it.copyTo(out) } }
                if (tmp.length() == 0L) {
                    tmp.delete()
                    throw TtsException(TtsFailure.EMPTY_AUDIO)
                }
                if (!tmp.renameTo(target)) {
                    target.delete()
                    tmp.renameTo(target)
                }
            }
        } catch (e: IOException) {
            throw TtsException(TtsFailure.NETWORK, cause = e)
        }
        target
    }

    /** Voices of a cloud engine: OpenAI = fixed list per model; ElevenLabs = `GET /v1/voices`; compatible = `GET <base>/audio/voices`. */
    suspend fun voices(engine: TtsEngine, settings: TtsSettings, apiKey: String?): List<TtsVoice> = withContext(Dispatchers.IO) {
        when (engine) {
            TtsEngine.SYSTEM -> emptyList()
            TtsEngine.OPENAI -> openAiVoices(settings.model.ifBlank { DEFAULT_OPENAI_MODEL }).map { TtsVoice(it, it.replaceFirstChar(Char::uppercase), null) }
            TtsEngine.ELEVENLABS -> {
                if (apiKey.isNullOrBlank()) return@withContext emptyList()
                val req = Request.Builder().url("$elevenLabsBase/voices").header("xi-api-key", apiKey.trim()).header("Accept", "application/json").get().build()
                runCatching { client.newCall(req).execute().use { r -> if (r.isSuccessful) parseElevenLabsVoices(r.body.string()) else emptyList() } }.getOrDefault(emptyList())
            }
            TtsEngine.OPENAI_COMPATIBLE -> {
                val base = compatibleBase(settings.endpoint) ?: return@withContext emptyList()
                val req = Request.Builder().url("$base/audio/voices").header("Accept", "application/json")
                    .apply { if (!apiKey.isNullOrBlank()) header("Authorization", "Bearer ${apiKey.trim()}") }.get().build()
                val listed = runCatching { client.newCall(req).execute().use { r -> if (r.isSuccessful) parseCompatibleVoices(r.body.string()) else emptyList() } }
                    .getOrDefault(emptyList())
                listed.ifEmpty { listOf(TtsVoice(DEFAULT_COMPATIBLE_VOICE, DEFAULT_COMPATIBLE_VOICE, null)) }
            }
        }
    }

    companion object {
        private val JSON = "application/json".toMediaType()
        const val DEFAULT_OPENAI_MODEL = "gpt-4o-mini-tts"
        const val DEFAULT_OPENAI_VOICE = "marin"
        const val DEFAULT_ELEVENLABS_MODEL = "eleven_multilingual_v2"
        const val DEFAULT_COMPATIBLE_MODEL = "kokoro"
        const val DEFAULT_COMPATIBLE_VOICE = "af_heart"
        val OPENAI_VOICES = listOf("alloy", "ash", "ballad", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer", "verse", "marin", "cedar")
        private val OPENAI_LEGACY_VOICES = listOf("alloy", "ash", "coral", "echo", "fable", "onyx", "nova", "sage", "shimmer")

        fun openAiVoices(model: String): List<String> = if (model == "tts-1" || model == "tts-1-hd") OPENAI_LEGACY_VOICES else OPENAI_VOICES

        /** `http://127.0.0.1:8880/v1` (or the full `…/audio/speech` URL of the classic app) → the base without trailing slash; null if invalid. */
        fun compatibleBase(endpoint: String): String? {
            val url = endpoint.trim().toHttpUrlOrNull() ?: return null
            if (url.username.isNotEmpty() || url.password.isNotEmpty()) return null
            val s = url.toString().trimEnd('/')
            return s.removeSuffix("/audio/speech").trimEnd('/')
        }

        /** The request of one paragraph (pure; throws [TtsException] when the settings are incomplete). */
        fun request(engine: TtsEngine, text: String, settings: TtsSettings, apiKey: String?, openAiBase: String, elevenLabsBase: String): TtsRequestSpec {
            val key = apiKey?.trim().orEmpty()
            return when (engine) {
                TtsEngine.SYSTEM -> throw TtsException(TtsFailure.ENGINE_UNAVAILABLE)
                TtsEngine.OPENAI -> {
                    if (key.isEmpty()) throw TtsException(TtsFailure.MISSING_KEY)
                    val model = settings.model.ifBlank { DEFAULT_OPENAI_MODEL }
                    val voice = settings.voice ?: DEFAULT_OPENAI_VOICE
                    if (voice !in openAiVoices(model)) throw TtsException(TtsFailure.MISSING_VOICE)
                    TtsRequestSpec(
                        url = "${openAiBase.trimEnd('/')}/audio/speech",
                        headers = mapOf("Authorization" to "Bearer $key", "Accept" to "audio/mpeg"),
                        body = buildJsonObject {
                            put("input", text)
                            put("model", model)
                            put("voice", voice)
                            put("response_format", "mp3")
                        }.toString(),
                    )
                }
                TtsEngine.ELEVENLABS -> {
                    if (key.isEmpty()) throw TtsException(TtsFailure.MISSING_KEY)
                    val voice = settings.voice ?: throw TtsException(TtsFailure.MISSING_VOICE)
                    TtsRequestSpec(
                        url = "${elevenLabsBase.trimEnd('/')}/text-to-speech/${java.net.URLEncoder.encode(voice, "UTF-8").replace("+", "%20")}?output_format=mp3_44100_128",
                        headers = mapOf("xi-api-key" to key, "Accept" to "audio/mpeg"),
                        body = buildJsonObject {
                            put("text", text)
                            put("model_id", settings.model.ifBlank { DEFAULT_ELEVENLABS_MODEL })
                        }.toString(),
                    )
                }
                TtsEngine.OPENAI_COMPATIBLE -> {
                    val base = compatibleBase(settings.endpoint) ?: throw TtsException(TtsFailure.BAD_ENDPOINT)
                    TtsRequestSpec(
                        url = "$base/audio/speech",
                        headers = buildMap {
                            put("Accept", "audio/mpeg")
                            if (key.isNotEmpty()) put("Authorization", "Bearer $key")
                        },
                        body = buildJsonObject {
                            put("input", text)
                            put("model", settings.model.ifBlank { DEFAULT_COMPATIBLE_MODEL })
                            put("voice", settings.voice ?: DEFAULT_COMPATIBLE_VOICE)
                            put("response_format", "mp3")
                        }.toString(),
                    )
                }
            }
        }

        /** Cache file name of one paragraph: sha256 of everything that changes the audio. */
        fun cacheKey(engine: TtsEngine, settings: TtsSettings, text: String): String {
            val material = listOf(engine.id, settings.endpoint.trim(), settings.model.trim(), settings.voice.orEmpty(), text).joinToString("\u0001")
            val digest = MessageDigest.getInstance("SHA-256").digest(material.toByteArray(Charsets.UTF_8))
            return digest.joinToString("") { "%02x".format(it) }
        }

        fun parseElevenLabsVoices(json: String): List<TtsVoice> = runCatching {
            val root = ReaderJson.parseToJsonElement(json) as? JsonObject
            root.list("voices").mapNotNull { v ->
                val id = v.str("voice_id") ?: return@mapNotNull null
                TtsVoice(id, v.str("name") ?: id, v.sub("labels").str("language") ?: v.sub("labels").str("accent"))
            }
        }.getOrDefault(emptyList())

        /** `{"voices":["af_heart",…]}` (Kokoro-FastAPI) or `{"voices":[{"id":…,"name":…}]}` or a bare array. */
        fun parseCompatibleVoices(json: String): List<TtsVoice> = runCatching {
            val root = ReaderJson.parseToJsonElement(json)
            val arr = (root as? JsonArray) ?: ((root as? JsonObject)?.get("voices") as? JsonArray) ?: ((root as? JsonObject)?.get("data") as? JsonArray)
            arr.orEmpty().mapNotNull { e ->
                when (e) {
                    is JsonPrimitive -> e.content.takeIf { it.isNotBlank() }?.let { TtsVoice(it, it, null) }
                    is JsonObject -> {
                        val id = e.str("id") ?: e.str("voice_id") ?: e.str("name") ?: return@mapNotNull null
                        TtsVoice(id, e.str("name") ?: id, e.str("language") ?: e.str("lang"))
                    }
                    else -> null
                }
            }
        }.getOrDefault(emptyList())
    }
}
