// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.SubtitleTranslation
import app.tatakai.next.model.TranslationProvider
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.concurrent.TimeUnit

// parity wave A (docs/redesign/15 §3.2, checklist player.subs.translate.*). Owner: worker p.
// Ported from the classic `src/lib/translation/providers.ts` (Google, DeepL / DeepLX, Azure, OpenAI-compatible) and the cue caches of
// `useSubtitleTranslation.ts` / `useSubtitlePretranslation.ts`. Google uses the public `translate_a/single?client=gtx` endpoint, which needs
// no key. The API key never comes from settings: the caller reads it from `SecretStore` (`SecretKeys.translationApiKey(provider.id)`) and
// hands it in [TranslatorConfig]; it is never logged.

/** Translation failed in a way the settings can fix (missing key / endpoint / region) or the provider refused. */
class TranslationException(val kind: Kind, message: String) : IOException(message) {
    enum class Kind { MISSING_KEY, MISSING_ENDPOINT, MISSING_REGION, HTTP, FORMAT, DISABLED }
}

/** Everything a translator needs; [apiKey] comes from `SecretStore`, never from `AppSettings`. */
data class TranslatorConfig(
    val provider: TranslationProvider,
    val targetLang: String,
    val endpoint: String = "",
    val model: String = "",
    val region: String = "",
    val apiKey: String? = null,
) {
    override fun toString(): String = "TranslatorConfig(provider=$provider, target=$targetLang, endpoint=$endpoint, model=$model, region=$region, key=${if (apiKey.isNullOrBlank()) "none" else "set"})"

    companion object {
        fun from(settings: SubtitleTranslation, apiKey: String?): TranslatorConfig =
            TranslatorConfig(settings.provider, settings.targetLang, settings.endpoint.trim(), settings.model.trim(), settings.region.trim(), apiKey?.trim())
    }
}

/** One translation backend. Returns exactly one output per input line, in order. */
fun interface SubtitleTranslator {
    suspend fun translate(lines: List<String>): List<String>
}

object SubtitleTranslators {
    const val GOOGLE_ENDPOINT = "https://translate.googleapis.com/translate_a/single"
    const val DEEPL_FREE_ENDPOINT = "https://api-free.deepl.com/v2/translate"
    const val DEEPL_PRO_ENDPOINT = "https://api.deepl.com/v2/translate"
    const val AZURE_ENDPOINT = "https://api.cognitive.microsofttranslator.com"

    /** Sample line of the settings "Probar" button. */
    const val TEST_LINE = "Hello! Where are you going?"

    /** Builds the translator of [config.provider]; throws [TranslationException] when a required field is missing. */
    fun create(config: TranslatorConfig, client: OkHttpClient): SubtitleTranslator {
        val http = client.newBuilder().callTimeout(20, TimeUnit.SECONDS).build()
        return when (config.provider) {
            TranslationProvider.NONE -> throw TranslationException(TranslationException.Kind.DISABLED, "translation is off")
            TranslationProvider.GOOGLE -> GoogleFreeTranslator(http, config.targetLang, config.endpoint.ifBlank { GOOGLE_ENDPOINT })
            TranslationProvider.DEEPL -> {
                val key = config.apiKey?.takeIf { it.isNotBlank() } ?: throw TranslationException(TranslationException.Kind.MISSING_KEY, "DeepL API key is required")
                // Free keys end with ":fx" and live on api-free; others on api.deepl.com (classic default: api-free).
                val default = if (key.endsWith(":fx")) DEEPL_FREE_ENDPOINT else DEEPL_PRO_ENDPOINT
                DeepLTranslator(http, config.targetLang, config.endpoint.ifBlank { default }, key)
            }
            TranslationProvider.DEEPLX -> {
                val url = config.endpoint.trim().trimEnd('/').ifBlank { throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "DeepLX endpoint URL is required") }
                DeepLxTranslator(http, config.targetLang, url, config.apiKey)
            }
            TranslationProvider.AZURE -> {
                val key = config.apiKey?.takeIf { it.isNotBlank() } ?: throw TranslationException(TranslationException.Kind.MISSING_KEY, "Azure Translator key is required")
                val region = config.region.ifBlank { throw TranslationException(TranslationException.Kind.MISSING_REGION, "Azure Translator region is required") }
                AzureTranslator(http, config.targetLang, config.endpoint.trim().trimEnd('/').ifBlank { AZURE_ENDPOINT }, key, region)
            }
            TranslationProvider.OPENAI -> {
                val url = completeOpenAiUrl(config.endpoint).ifBlank { throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "OpenAI-compatible base URL is required") }
                OpenAiTranslator(http, config.targetLang, url, config.apiKey, config.model)
            }
        }
    }

    /** Classic `completeOpenAICompatUrl`: a pasted base URL becomes its `/chat/completions` endpoint. */
    fun completeOpenAiUrl(url: String): String {
        val cleaned = url.trim().trimEnd('/')
        if (cleaned.isEmpty() || cleaned.endsWith("/chat/completions")) return cleaned
        if (Regex("/v\\d+/(responses|completions)$").containsMatchIn(cleaned)) return cleaned.replace(Regex("/(responses|completions)$"), "/chat/completions")
        if (Regex("/v\\d+$").containsMatchIn(cleaned)) return "$cleaned/chat/completions"
        val parsed = cleaned.toHttpUrlOrNull() ?: return cleaned
        if (parsed.encodedPath == "/" || parsed.encodedPath.isEmpty()) return "${parsed.scheme}://${parsed.host}${if (parsed.port != okhttp3.HttpUrl.defaultPort(parsed.scheme)) ":${parsed.port}" else ""}/v1/chat/completions"
        return cleaned
    }

    /** Classic DEEPL_TARGET_MAP. */
    fun deeplTarget(lang: String): String = when (lang.lowercase()) {
        "en" -> "EN-US"
        "zh" -> "ZH-HANS"
        "zh-hant" -> "ZH-HANT"
        "pt" -> "PT-PT"
        "pt-br" -> "PT-BR"
        else -> lang.uppercase()
    }

    /** Classic AZURE_LANG_MAP. */
    fun azureCode(lang: String): String = when (lang.lowercase()) {
        "tl" -> "fil"
        "no" -> "nb"
        "zh" -> "zh-Hans"
        "zh-hant" -> "zh-Hant"
        else -> lang
    }

    /** Classic GOOGLE_LANG_MAP. */
    fun googleCode(lang: String): String = when (lang.lowercase()) {
        "zh-hant" -> "zh-TW"
        "zh" -> "zh-CN"
        else -> lang
    }

    /** English display name for prompts (classic `getTranslationLanguageName`). */
    fun languageName(code: String): String = LANGUAGE_NAMES[code.lowercase()] ?: code.uppercase()

    /** Target languages offered by the settings picker (classic TRANSLATION_LANGUAGES, the common ones). */
    val TARGET_LANGUAGES: List<String> = listOf("es", "en", "pt", "fr", "de", "it", "ja", "ko", "zh", "ru", "ar", "id", "tr", "pl")

    private val LANGUAGE_NAMES = mapOf(
        "en" to "English", "es" to "Spanish", "ja" to "Japanese", "fr" to "French", "de" to "German", "pt" to "Portuguese",
        "it" to "Italian", "ru" to "Russian", "ko" to "Korean", "zh" to "Chinese (Simplified)", "zh-hant" to "Chinese (Traditional)",
        "ar" to "Arabic", "id" to "Indonesian", "tr" to "Turkish", "pl" to "Polish", "nl" to "Dutch", "vi" to "Vietnamese", "th" to "Thai",
    )
}

private val JSON_MEDIA = "application/json; charset=utf-8".toMediaType()
private val JSON = Json { ignoreUnknownKeys = true }

private suspend fun OkHttpClient.postJson(url: String, body: JsonElement, headers: Map<String, String> = emptyMap()): JsonElement =
    withContext(Dispatchers.IO) {
        val req = Request.Builder().url(url).post(body.toString().toRequestBody(JSON_MEDIA)).apply { headers.forEach { (k, v) -> header(k, v) } }.build()
        newCall(req).execute().use { res ->
            val text = res.body.string()
            if (!res.isSuccessful) throw TranslationException(TranslationException.Kind.HTTP, "HTTP ${res.code}${errorMessage(text)?.let { ": $it" }.orEmpty()}")
            runCatching { JSON.parseToJsonElement(text) }.getOrElse { throw TranslationException(TranslationException.Kind.FORMAT, "not JSON") }
        }
    }

/** `{error:{message}}` / `{message}` of an error body, never the request (no key echo). */
private fun errorMessage(text: String): String? {
    val obj = runCatching { JSON.parseToJsonElement(text) as? JsonObject }.getOrNull() ?: return null
    val err = obj["error"]
    val msg = (err as? JsonObject)?.get("message")?.jsonPrimitive?.contentOrNull
        ?: (err as? JsonPrimitive)?.contentOrNull
        ?: obj["message"]?.jsonPrimitive?.contentOrNull
    return msg?.take(160)
}

/** Joins the lines with '\n' for providers that take one text, and splits the answer back; null when the count changed. */
private fun splitBack(original: List<String>, translated: String): List<String>? {
    val parts = translated.split('\n')
    return if (parts.size == original.size) parts else null
}

/** Multi-line cues travel as one line each (a cue's own line breaks are encoded so batching does not shift lines). */
private const val CUE_BREAK = " \u2028 "

private fun encodeCue(s: String) = s.replace("\n", CUE_BREAK)

private fun decodeCue(s: String) = s.replace(Regex("\\s*\u2028\\s*"), "\n").trim()

/** Google `translate_a/single?client=gtx` (no key, form POST so long batches fit). Response: `[[["translated","original",...],...],...]`. */
internal class GoogleFreeTranslator(private val client: OkHttpClient, private val target: String, private val endpoint: String) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        val joined = lines.joinToString("\n") { encodeCue(it) }
        val url = endpoint.toHttpUrlOrNull()?.newBuilder()
            ?.addQueryParameter("client", "gtx")?.addQueryParameter("sl", "auto")
            ?.addQueryParameter("tl", SubtitleTranslators.googleCode(target))?.addQueryParameter("dt", "t")
            ?.build() ?: throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "bad endpoint")
        val text = withContext(Dispatchers.IO) {
            val form = okhttp3.FormBody.Builder().add("q", joined).build()
            client.newCall(Request.Builder().url(url).post(form).build()).execute().use { res ->
                val body = res.body.string()
                if (!res.isSuccessful) throw TranslationException(TranslationException.Kind.HTTP, "HTTP ${res.code}")
                body
            }
        }
        val root = runCatching { JSON.parseToJsonElement(text).jsonArray }.getOrElse { throw TranslationException(TranslationException.Kind.FORMAT, "Google: not an array") }
        val segments = root.firstOrNull() as? JsonArray ?: throw TranslationException(TranslationException.Kind.FORMAT, "Google: no segments")
        val out = segments.joinToString("") { seg -> ((seg as? JsonArray)?.firstOrNull() as? JsonPrimitive)?.contentOrNull.orEmpty() }
        return (splitBack(lines, out) ?: if (lines.size == 1) listOf(out) else null)?.map(::decodeCue)
            ?: lines.map { translate(listOf(it)).single() }
    }
}

/** DeepL v2: `{"text":[...], "target_lang":"ES"}` with `Authorization: DeepL-Auth-Key <key>` -> `{"translations":[{"text"}]}`. */
internal class DeepLTranslator(private val client: OkHttpClient, private val target: String, private val endpoint: String, private val key: String) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        val body = buildJsonObject {
            putJsonArray("text") { lines.forEach { add(it) } }
            put("target_lang", SubtitleTranslators.deeplTarget(target))
        }
        val res = client.postJson(endpoint, body, mapOf("Authorization" to "DeepL-Auth-Key $key"))
        val list = (res as? JsonObject)?.get("translations")?.jsonArray
            ?: throw TranslationException(TranslationException.Kind.FORMAT, "DeepL: no translations")
        val out = list.map { it.jsonObject["text"]?.jsonPrimitive?.contentOrNull ?: throw TranslationException(TranslationException.Kind.FORMAT, "DeepL: no text") }
        if (out.size != lines.size) throw TranslationException(TranslationException.Kind.FORMAT, "DeepL: count mismatch")
        return out
    }
}

/** DeepLX (self-hosted): `{"text":"...","target_lang":"ES"}` -> `{"data":"..."}`; an optional token goes as `Authorization: Bearer`. */
internal class DeepLxTranslator(private val client: OkHttpClient, private val target: String, private val endpoint: String, private val token: String?) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        val joined = lines.joinToString("\n") { encodeCue(it) }
        val body = buildJsonObject {
            put("text", joined)
            put("source_lang", "auto")
            put("target_lang", SubtitleTranslators.deeplTarget(target))
        }
        val headers = token?.takeIf { it.isNotBlank() }?.let { mapOf("Authorization" to "Bearer $it") }.orEmpty()
        val res = client.postJson(endpoint, body, headers)
        val data = (res as? JsonObject)?.get("data")?.jsonPrimitive?.contentOrNull
            ?: throw TranslationException(TranslationException.Kind.FORMAT, "DeepLX: no data")
        return (splitBack(lines, data) ?: if (lines.size == 1) listOf(data) else null)?.map(::decodeCue)
            ?: lines.map { translate(listOf(it)).single() }
    }
}

/** Azure Translator v3: `POST /translate?api-version=3.0&to=es` with `[{"Text":...}]` -> `[{"translations":[{"text"}]}]`. */
internal class AzureTranslator(
    private val client: OkHttpClient,
    private val target: String,
    private val endpoint: String,
    private val key: String,
    private val region: String,
) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        val url = "$endpoint/translate".toHttpUrlOrNull()?.newBuilder()
            ?.addQueryParameter("api-version", "3.0")?.addQueryParameter("to", SubtitleTranslators.azureCode(target))?.build()
            ?: throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "bad endpoint")
        val body = buildJsonArray { lines.forEach { line -> addJsonObject { put("Text", line) } } }
        val res = client.postJson(url.toString(), body, mapOf("Ocp-Apim-Subscription-Key" to key, "Ocp-Apim-Subscription-Region" to region))
        val arr = res as? JsonArray ?: throw TranslationException(TranslationException.Kind.FORMAT, "Azure: not an array")
        val out = arr.map { item ->
            item.jsonObject["translations"]?.jsonArray?.firstOrNull()?.jsonObject?.get("text")?.jsonPrimitive?.contentOrNull
                ?: throw TranslationException(TranslationException.Kind.FORMAT, "Azure: no text")
        }
        if (out.size != lines.size) throw TranslationException(TranslationException.Kind.FORMAT, "Azure: count mismatch")
        return out
    }
}

/** OpenAI-compatible chat completions (OpenAI, DeepSeek, Ollama, LM Studio...): numbered lines in, numbered lines out. */
internal class OpenAiTranslator(
    private val client: OkHttpClient,
    private val target: String,
    private val endpoint: String,
    private val key: String?,
    private val model: String,
) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        val numbered = lines.mapIndexed { i, l -> "${i + 1}. ${encodeCue(l)}" }.joinToString("\n")
        val maxTokens = (numbered.length * 2 + 256).coerceIn(256, 4096)
        val body = buildJsonObject {
            if (model.isNotBlank()) put("model", model)
            put("temperature", 0)
            put("max_tokens", maxTokens)
            put("stream", false)
            putJsonArray("messages") {
                addJsonObject {
                    put("role", "system")
                    put(
                        "content",
                        "You are a professional subtitle translator. Translate every numbered line into ${SubtitleTranslators.languageName(target)}. " +
                            "Answer with the same numbered lines, one per line, in the same order, and nothing else. Keep the \u2028 marks.",
                    )
                }
                addJsonObject {
                    put("role", "user")
                    put("content", numbered)
                }
            }
        }
        val headers = key?.takeIf { it.isNotBlank() }?.let { mapOf("Authorization" to "Bearer $it") }.orEmpty()
        val res = client.postJson(endpoint, body, headers)
        val choice = (res as? JsonObject)?.get("choices")?.jsonArray?.firstOrNull()?.jsonObject
        val content = choice?.get("message")?.jsonObject?.get("content")?.jsonPrimitive?.contentOrNull
            ?: throw TranslationException(TranslationException.Kind.FORMAT, "OpenAI: no content")
        if (choice["finish_reason"]?.jsonPrimitive?.contentOrNull == "length") throw TranslationException(TranslationException.Kind.FORMAT, "OpenAI: truncated")
        val cleaned = content.replace(Regex("^(\\s*<think>[\\s\\S]*?</think>)+\\s*", RegexOption.IGNORE_CASE), "").trim()
        return parseNumbered(cleaned, lines.size)?.map(::decodeCue)
            ?: if (lines.size == 1) listOf(decodeCue(cleaned.removePrefix("1.").trim())) else lines.map { translate(listOf(it)).single() }
    }

    companion object {
        private val NUMBERED = Regex("""^\s*(\d+)[.)]\s?(.*)$""")

        /** `1. Hola\n2. Adiós` -> ["Hola", "Adiós"]; null when a number is missing. */
        fun parseNumbered(text: String, count: Int): List<String>? {
            val byNumber = HashMap<Int, String>()
            for (line in text.split('\n')) {
                val m = NUMBERED.find(line) ?: continue
                val n = m.groupValues[1].toIntOrNull() ?: continue
                if (n in 1..count && n !in byNumber) byNumber[n] = m.groupValues[2].trim()
            }
            return if (byNumber.size == count) (1..count).map { byNumber.getValue(it) } else null
        }
    }
}

/**
 * Translation cache (player.subs.translate.cache): memory + one JSON file per episode and target language under
 * `cacheDir/subs-translate/` (`original -> translated`). Re-watching a translated episode never calls the provider again; "Vaciar
 * caché" deletes the folder. Thread-safe.
 */
class TranslationCache(private val dir: File) {
    private val mutex = Mutex()
    private val memory = HashMap<String, MutableMap<String, String>>()

    private fun fileOf(scope: String) = File(dir, sha1(scope).take(32) + ".json")

    private suspend fun table(scope: String): MutableMap<String, String> = memory[scope] ?: withContext(Dispatchers.IO) {
        val f = fileOf(scope)
        val loaded = if (f.isFile) {
            runCatching { JSON.parseToJsonElement(f.readText()).jsonObject.mapValues { it.value.jsonPrimitive.content } }.getOrDefault(emptyMap())
        } else {
            emptyMap()
        }
        LinkedHashMap(loaded)
    }.also { memory[scope] = it }

    suspend fun get(scope: String, lines: List<String>): List<String?> = mutex.withLock {
        val t = table(scope)
        lines.map { t[it] }
    }

    suspend fun put(scope: String, pairs: Map<String, String>) = mutex.withLock {
        if (pairs.isEmpty()) return@withLock
        val t = table(scope)
        t.putAll(pairs)
        // Bounded like the classic cue cache (FIFO-ish: oldest insertion order is lost on reload, the cap keeps files small).
        while (t.size > MAX_ENTRIES_PER_EPISODE) t.remove(t.keys.first())
        withContext(Dispatchers.IO) {
            dir.mkdirs()
            val json = buildJsonObject { t.forEach { (k, v) -> put(k, v) } }
            val tmp = File(dir, fileOf(scope).name + ".tmp")
            tmp.writeText(json.toString())
            if (!tmp.renameTo(fileOf(scope))) {
                fileOf(scope).writeText(json.toString())
                tmp.delete()
            }
        }
    }

    /** "Vaciar caché". */
    suspend fun clear() = mutex.withLock {
        memory.clear()
        withContext(Dispatchers.IO) { dir.listFiles()?.forEach { it.delete() } }
    }

    /** Bytes on disk (shown next to "Vaciar caché"). */
    fun sizeBytes(): Long = dir.listFiles()?.sumOf { it.length() } ?: 0L

    companion object {
        const val MAX_ENTRIES_PER_EPISODE = 5_000

        /** Cache scope of one episode in one target language with one provider. */
        fun scope(episodeKey: String, config: TranslatorConfig): String = "${config.provider.id}|${config.targetLang}|$episodeKey"

        private fun sha1(s: String): String =
            MessageDigest.getInstance("SHA-1").digest(s.toByteArray()).joinToString("") { "%02x".format(it) }
    }
}

/**
 * Live + preload translation over one [SubtitleTranslator] and the [TranslationCache] of one episode. Live: [translateLive] returns the
 * cached lines at once or translates the missing ones (classic: < 2 s per cue on a normal connection). Preload: [pretranslate] sends the
 * whole track in batches of at most 40 lines / 3000 characters (classic BATCH_MAX_*), reporting progress; a failed batch is skipped and
 * left to the live path.
 */
class SubtitleTranslationSession(
    private val translator: SubtitleTranslator,
    private val cache: TranslationCache,
    private val scope: String,
) {
    suspend fun translateLive(lines: List<String>): List<String> {
        val wanted = lines.filter { it.isNotBlank() }
        if (wanted.isEmpty()) return lines
        val cached = cache.get(scope, wanted)
        val missing = wanted.filterIndexed { i, _ -> cached[i] == null }.distinct()
        val fresh = if (missing.isEmpty()) emptyMap() else missing.zip(translator.translate(missing)).toMap().also { cache.put(scope, it) }
        val all = cache.get(scope, lines)
        return lines.mapIndexed { i, l -> all[i] ?: fresh[l] ?: l }
    }

    /** Only cached lines, null for the others (no network): what the screen shows while a live request is in flight. */
    suspend fun cachedOnly(lines: List<String>): List<String?> = cache.get(scope, lines)

    /** Translates every text not cached yet; [onProgress] gets (done, total) after each batch. Returns how many lines failed. */
    suspend fun pretranslate(texts: List<String>, onProgress: (done: Int, total: Int) -> Unit): Int {
        val unique = texts.filter { it.isNotBlank() }.distinct()
        val cached = cache.get(scope, unique)
        val todo = unique.filterIndexed { i, _ -> cached[i] == null }
        var done = unique.size - todo.size
        var failed = 0
        onProgress(done, unique.size)
        for (batch in batches(todo)) {
            try {
                cache.put(scope, batch.zip(translator.translate(batch)).toMap())
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                failed += batch.size
            }
            done += batch.size
            onProgress(done, unique.size)
        }
        return failed
    }

    companion object {
        const val BATCH_MAX_LINES = 40
        const val BATCH_MAX_CHARS = 3000

        fun batches(texts: List<String>): List<List<String>> {
            val out = ArrayList<List<String>>()
            var batch = ArrayList<String>()
            var chars = 0
            for (t in texts) {
                if (batch.isNotEmpty() && (batch.size >= BATCH_MAX_LINES || chars + t.length > BATCH_MAX_CHARS)) {
                    out += batch
                    batch = ArrayList()
                    chars = 0
                }
                batch += t
                chars += t.length
            }
            if (batch.isNotEmpty()) out += batch
            return out
        }
    }
}
