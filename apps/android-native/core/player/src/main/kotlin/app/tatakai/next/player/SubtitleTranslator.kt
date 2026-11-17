// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.SubtitleTranslation
import app.tatakai.next.model.TranslationProvider
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
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
import okhttp3.FormBody
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import java.io.File
import java.io.IOException
import java.security.MessageDigest
import java.util.concurrent.TimeUnit

// parity wave A (docs/redesign/15 §3.2, checklist player.subs.translate.*). Owner: worker p.
// Ported from the classic `src/lib/translation/providers.ts` (Google, DeepL / DeepLX, Azure, OpenAI-compatible) and the cue caches of
// `useSubtitleTranslation.ts` / `useSubtitlePretranslation.ts`. Google uses the public `translate_a/single?client=gtx` endpoint, which needs
// no key. The API key never comes from settings: the caller reads it from `SecretStore` (`SecretKeys.translationApiKey(provider.id)`) and
// hands it in [TranslatorConfig]; it is never logged.
//
// 0.4.0 (sub-translation worker): every request goes through a [RequestGate] (pacing + 429 / 503 back-off, see TranslationThrottle.kt);
// keyless Google sends a browser User-Agent, uses GET for short batches and fails over to `clients5 ... dict-chrome-ex` while
// `translate.googleapis.com` is rate limited; a batch whose line count comes back wrong is split in halves instead of being re-sent
// line by line (that fallback alone used to fire 40 requests at once and trigger the 429s).

/**
 * Translation failed in a way the settings can fix (missing key / endpoint / region) or the provider refused. [Kind.RATE_LIMITED]
 * (0.4.0) = the provider said "too many requests" (HTTP 429 / 503) or its [RequestGate] is still cooling down: nothing is wrong with
 * the lines, try again after [retryAfterMs] and keep showing the originals meanwhile.
 */
class TranslationException(val kind: Kind, message: String, val retryAfterMs: Long? = null) : IOException(message) {
    enum class Kind { MISSING_KEY, MISSING_ENDPOINT, MISSING_REGION, HTTP, FORMAT, DISABLED, RATE_LIMITED }

    companion object {
        fun rateLimited(retryAfterMs: Long) =
            TranslationException(Kind.RATE_LIMITED, "rate limited, retry in ${(retryAfterMs + 999) / 1000} s", retryAfterMs.coerceAtLeast(0))
    }
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

    /** 0.4.0: second keyless Google endpoint (the "Google Translate" Chrome extension's), used while the first one answers 429. */
    const val GOOGLE_DICT_ENDPOINT = "https://clients5.google.com/translate_a/t"
    const val DEEPL_FREE_ENDPOINT = "https://api-free.deepl.com/v2/translate"
    const val DEEPL_PRO_ENDPOINT = "https://api.deepl.com/v2/translate"
    const val AZURE_ENDPOINT = "https://api.cognitive.microsofttranslator.com"

    /** Sample line of the settings "Probar" button. */
    const val TEST_LINE = "Hello! Where are you going?"

    /** A normal browser User-Agent: the keyless Google endpoints throttle unknown clients (`okhttp/x`) much sooner. */
    const val BROWSER_USER_AGENT =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"

    /** Process-wide [RequestGate] of an endpoint: keyless Google shares one budget per host; keyed providers get a lighter gate. */
    fun defaultGate(provider: TranslationProvider, url: String): RequestGate {
        val host = url.toHttpUrlOrNull()?.let { "${it.host}:${it.port}" } ?: url
        return if (provider == TranslationProvider.GOOGLE) {
            RequestGate.shared("google|$host") { RequestGate.googleFree() }
        } else {
            RequestGate.shared("${provider.id}|$host") { RequestGate.keyed() }
        }
    }

    /**
     * Builds the translator of [config.provider]; throws [TranslationException] when a required field is missing. [gate] picks the
     * pacing / back-off state of an endpoint URL (tests inject fresh ones on virtual time).
     */
    fun create(
        config: TranslatorConfig,
        client: OkHttpClient,
        gate: (provider: TranslationProvider, url: String) -> RequestGate = ::defaultGate,
    ): SubtitleTranslator {
        val http = client.newBuilder().callTimeout(20, TimeUnit.SECONDS).build()
        return when (config.provider) {
            TranslationProvider.NONE -> throw TranslationException(TranslationException.Kind.DISABLED, "translation is off")
            TranslationProvider.GOOGLE -> {
                // A custom endpoint (power users, tests) is used alone; the default pair fails over between each other on 429.
                val endpoints = config.endpoint.trim().takeIf { it.isNotEmpty() }?.let { listOf(GoogleEndpoint.of(it)) }
                    ?: listOf(GoogleEndpoint(GOOGLE_ENDPOINT, GoogleShape.SINGLE), GoogleEndpoint(GOOGLE_DICT_ENDPOINT, GoogleShape.DICT))
                GoogleFreeTranslator(http, config.targetLang, endpoints) { gate(TranslationProvider.GOOGLE, it.url) }
            }
            TranslationProvider.DEEPL -> {
                val key = config.apiKey?.takeIf { it.isNotBlank() } ?: throw TranslationException(TranslationException.Kind.MISSING_KEY, "DeepL API key is required")
                // Free keys end with ":fx" and live on api-free; others on api.deepl.com (classic default: api-free).
                val default = if (key.endsWith(":fx")) DEEPL_FREE_ENDPOINT else DEEPL_PRO_ENDPOINT
                val url = config.endpoint.ifBlank { default }
                DeepLTranslator(http, config.targetLang, url, key, gate(config.provider, url))
            }
            TranslationProvider.DEEPLX -> {
                val url = config.endpoint.trim().trimEnd('/').ifBlank { throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "DeepLX endpoint URL is required") }
                DeepLxTranslator(http, config.targetLang, url, config.apiKey, gate(config.provider, url))
            }
            TranslationProvider.AZURE -> {
                val key = config.apiKey?.takeIf { it.isNotBlank() } ?: throw TranslationException(TranslationException.Kind.MISSING_KEY, "Azure Translator key is required")
                val region = config.region.ifBlank { throw TranslationException(TranslationException.Kind.MISSING_REGION, "Azure Translator region is required") }
                val url = config.endpoint.trim().trimEnd('/').ifBlank { AZURE_ENDPOINT }
                AzureTranslator(http, config.targetLang, url, key, region, gate(config.provider, url))
            }
            TranslationProvider.OPENAI -> {
                val url = completeOpenAiUrl(config.endpoint).ifBlank { throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "OpenAI-compatible base URL is required") }
                OpenAiTranslator(http, config.targetLang, url, config.apiKey, config.model, gate(config.provider, url))
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
        if (parsed.encodedPath == "/" || parsed.encodedPath.isEmpty()) return "${parsed.scheme}://${parsed.host}${if (parsed.port != HttpUrl.defaultPort(parsed.scheme)) ":${parsed.port}" else ""}/v1/chat/completions"
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

/** HTTP statuses that mean "slow down" (DeepL's 456 = quota exhausted is not one of them: waiting does not help). */
private fun Int.isThrottle() = this == 429 || this == 503

/** `Retry-After` (seconds / HTTP-date) or OpenAI's `retry-after-ms`. */
private fun Response.retryAfterMs(): Long? =
    header("retry-after-ms")?.trim()?.toLongOrNull() ?: RetryAfter.parseMs(header("Retry-After"))

/**
 * Runs [request] through [gate]: waits for a slot, opens the breaker on 429 / 503 (throwing RATE_LIMITED with its wait) and closes it
 * on success. Returns the body of a successful answer; other HTTP errors throw [TranslationException.Kind.HTTP].
 */
private suspend fun OkHttpClient.gated(gate: RequestGate, request: Request, describe: (code: Int, body: String) -> String = { c, _ -> "HTTP $c" }): String {
    gate.acquire()
    val (code, body, retry) = withContext(Dispatchers.IO) {
        newCall(request).execute().use { res -> Triple(res.code, res.body.string(), if (res.code.isThrottle()) res.retryAfterMs() else null) }
    }
    if (code.isThrottle()) throw TranslationException.rateLimited(gate.onRateLimited(retry))
    if (code !in 200..299) throw TranslationException(TranslationException.Kind.HTTP, describe(code, body))
    gate.onSuccess()
    return body
}

private suspend fun OkHttpClient.postJson(url: String, body: JsonElement, gate: RequestGate, headers: Map<String, String> = emptyMap()): JsonElement {
    val req = Request.Builder().url(url).post(body.toString().toRequestBody(JSON_MEDIA)).apply { headers.forEach { (k, v) -> header(k, v) } }.build()
    val text = gated(gate, req) { code, b -> "HTTP $code${errorMessage(b)?.let { ": $it" }.orEmpty()}" }
    return runCatching { JSON.parseToJsonElement(text) }.getOrElse { throw TranslationException(TranslationException.Kind.FORMAT, "not JSON") }
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
    val parts = translated.trimEnd('\n').split('\n')
    return if (parts.size == original.size) parts else null
}

/** Multi-line cues travel as one line each (a cue's own line breaks are encoded so batching does not shift lines). */
private const val CUE_BREAK = " \u2028 "

private fun encodeCue(s: String) = s.replace("\n", CUE_BREAK)

private fun decodeCue(s: String) = s.replace(Regex("\\s*\u2028\\s*"), "\n").trim()

/**
 * A batch whose answer could not be split back line by line: translate both halves (log2(n) extra requests at most, all paced by the
 * gate) instead of one request per line.
 */
private suspend fun SubtitleTranslator.halves(lines: List<String>): List<String> {
    val mid = lines.size / 2
    return translate(lines.subList(0, mid)) + translate(lines.subList(mid, lines.size))
}

/** Response shape of a keyless Google endpoint. */
enum class GoogleShape {
    /** `translate_a/single?client=gtx&dt=t`: `[[["translated","original",...],...],null,"en",...]` for one `q`. */
    SINGLE,

    /** `translate_a/t?client=dict-chrome-ex`: one result per `q` (see [GoogleFreeTranslator.parseDict]). */
    DICT,
}

data class GoogleEndpoint(val url: String, val shape: GoogleShape) {
    companion object {
        fun of(url: String) = GoogleEndpoint(url, if (url.contains("translate_a/t")) GoogleShape.DICT else GoogleShape.SINGLE)
    }
}

/**
 * Keyless Google over [endpoints] in order of preference. Each endpoint has its own [RequestGate]: while one is rate limited (breaker
 * open) the next is used; when all are, RATE_LIMITED is thrown with the shortest wait so the caller can retry then.
 */
internal class GoogleFreeTranslator(
    private val client: OkHttpClient,
    private val target: String,
    private val endpoints: List<GoogleEndpoint>,
    private val gateOf: (GoogleEndpoint) -> RequestGate,
) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        var wait = Long.MAX_VALUE
        var formatError: TranslationException? = null
        for (ep in endpoints) {
            val gate = gateOf(ep)
            val cooling = gate.cooldownMs()
            if (cooling > 0) {
                wait = minOf(wait, cooling)
                continue
            }
            try {
                return when (ep.shape) {
                    GoogleShape.SINGLE -> single(ep, gate, lines)
                    GoogleShape.DICT -> dict(ep, gate, lines)
                }
            } catch (e: TranslationException) {
                when (e.kind) {
                    TranslationException.Kind.RATE_LIMITED -> wait = minOf(wait, e.retryAfterMs ?: 0)
                    // An endpoint that changed its JSON must not take the feature down while the other one still works.
                    TranslationException.Kind.FORMAT -> formatError = e
                    else -> throw e
                }
            }
        }
        if (wait != Long.MAX_VALUE) throw TranslationException.rateLimited(wait)
        throw formatError ?: TranslationException(TranslationException.Kind.FORMAT, "Google: no endpoint")
    }

    private fun request(url: HttpUrl, form: List<Pair<String, String>>): Request {
        // GET for short batches (what the browser extension does), form POST when the URL would be too long.
        val getUrl = url.newBuilder().apply { form.forEach { (k, v) -> addQueryParameter(k, v) } }.build()
        val builder = Request.Builder()
            .header("User-Agent", SubtitleTranslators.BROWSER_USER_AGENT)
            .header("Accept", "*/*")
        return if (getUrl.toString().length <= MAX_GET_URL) {
            builder.url(getUrl).get().build()
        } else {
            builder.url(url).post(FormBody.Builder().apply { form.forEach { (k, v) -> add(k, v) } }.build()).build()
        }
    }

    private suspend fun single(ep: GoogleEndpoint, gate: RequestGate, lines: List<String>): List<String> {
        val base = ep.url.toHttpUrlOrNull()?.newBuilder()
            ?.addQueryParameter("client", "gtx")?.addQueryParameter("sl", "auto")
            ?.addQueryParameter("tl", SubtitleTranslators.googleCode(target))?.addQueryParameter("dt", "t")
            ?.build() ?: throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "bad endpoint")
        val text = client.gated(gate, request(base, listOf("q" to lines.joinToString("\n") { encodeCue(it) })))
        val out = parseSingle(text)
        return (splitBack(lines, out) ?: if (lines.size == 1) listOf(out) else null)?.map(::decodeCue)
            ?: halves(lines)
    }

    private suspend fun dict(ep: GoogleEndpoint, gate: RequestGate, lines: List<String>): List<String> {
        val base = ep.url.toHttpUrlOrNull()?.newBuilder()
            ?.addQueryParameter("client", "dict-chrome-ex")?.addQueryParameter("sl", "auto")
            ?.addQueryParameter("tl", SubtitleTranslators.googleCode(target))
            ?.build() ?: throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "bad endpoint")
        // One `q` per line: this endpoint answers one result per q, so no newline protocol is needed between cues.
        val text = client.gated(gate, request(base, lines.map { "q" to encodeCue(it) }))
        return parseDict(text, lines.size)?.map(::decodeCue)
            ?: if (lines.size > 1) halves(lines) else throw TranslationException(TranslationException.Kind.FORMAT, "Google dict: unexpected answer")
    }

    companion object {
        /** Conservative: some proxies / Google front ends refuse URLs over ~2 KB. */
        const val MAX_GET_URL = 1_900

        /** gtx: concatenation of the first string of every segment of `root[0]`. */
        fun parseSingle(text: String): String {
            val root = runCatching { JSON.parseToJsonElement(text).jsonArray }.getOrElse { throw TranslationException(TranslationException.Kind.FORMAT, "Google: not an array") }
            val segments = root.firstOrNull() as? JsonArray ?: throw TranslationException(TranslationException.Kind.FORMAT, "Google: no segments")
            return segments.joinToString("") { seg -> ((seg as? JsonArray)?.firstOrNull() as? JsonPrimitive)?.contentOrNull.orEmpty() }
        }

        /**
         * dict-chrome-ex answers, by era / parameters (all seen in the wild; null when none fits [count]):
         * - `[["Hola","en"],["Adiós","en"]]` - one `[translation, detected source]` per q (sl=auto, current);
         * - `["Hola","Adiós"]` - one string per q (fixed sl), or `["Hola","en"]` for a single q with sl=auto;
         * - `"Hola"` - a bare string for a single q;
         * - `{"sentences":[{"trans":"Hola","orig":"Hello"}],"src":"en"}` - the older object form (single q).
         */
        fun parseDict(text: String, count: Int): List<String>? {
            val root = runCatching { JSON.parseToJsonElement(text) }.getOrNull() ?: return null
            val out: List<String>? = when (root) {
                is JsonPrimitive -> root.takeIf { it.isString && count == 1 }?.contentOrNull?.let { listOf(it) }
                is JsonObject -> {
                    val joined = (root["sentences"] as? JsonArray)?.joinToString("") { s ->
                        ((s as? JsonObject)?.get("trans") as? JsonPrimitive)?.contentOrNull.orEmpty()
                    }
                    if (joined == null) null else if (count == 1) listOf(joined) else splitBack(List(count) { "" }, joined)
                }
                is JsonArray -> when {
                    root.isEmpty() -> null
                    root.all { it is JsonArray } -> root.map { ((it as JsonArray).firstOrNull() as? JsonPrimitive)?.contentOrNull ?: return null }
                        .takeIf { it.size == count }
                    root.all { it is JsonPrimitive } -> {
                        val strings = root.map { (it as JsonPrimitive).contentOrNull ?: return null }
                        when {
                            strings.size == count -> strings
                            count == 1 -> listOf(strings.first())
                            else -> null
                        }
                    }
                    else -> null
                }
            }
            return out?.map(::unescapeHtml)
        }

        private val NUMERIC_ENTITY = Regex("&#(x?)([0-9a-fA-F]+);")

        /** dict-chrome-ex may HTML-escape its output (`&#39;`, `&quot;`). */
        internal fun unescapeHtml(s: String): String {
            if ('&' !in s) return s
            return s.replace(NUMERIC_ENTITY) { m ->
                val cp = m.groupValues[2].toIntOrNull(if (m.groupValues[1].isEmpty()) 10 else 16)
                if (cp != null && Character.isValidCodePoint(cp)) String(Character.toChars(cp)) else m.value
            }.replace("&quot;", "\"").replace("&apos;", "'").replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&")
        }
    }
}

/** DeepL v2: `{"text":[...], "target_lang":"ES"}` with `Authorization: DeepL-Auth-Key <key>` -> `{"translations":[{"text"}]}`. */
internal class DeepLTranslator(
    private val client: OkHttpClient,
    private val target: String,
    private val endpoint: String,
    private val key: String,
    private val gate: RequestGate,
) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        val body = buildJsonObject {
            putJsonArray("text") { lines.forEach { add(it) } }
            put("target_lang", SubtitleTranslators.deeplTarget(target))
        }
        val res = client.postJson(endpoint, body, gate, mapOf("Authorization" to "DeepL-Auth-Key $key"))
        val list = (res as? JsonObject)?.get("translations")?.jsonArray
            ?: throw TranslationException(TranslationException.Kind.FORMAT, "DeepL: no translations")
        val out = list.map { it.jsonObject["text"]?.jsonPrimitive?.contentOrNull ?: throw TranslationException(TranslationException.Kind.FORMAT, "DeepL: no text") }
        if (out.size != lines.size) throw TranslationException(TranslationException.Kind.FORMAT, "DeepL: count mismatch")
        return out
    }
}

/** DeepLX (self-hosted): `{"text":"...","target_lang":"ES"}` -> `{"data":"..."}`; an optional token goes as `Authorization: Bearer`. */
internal class DeepLxTranslator(
    private val client: OkHttpClient,
    private val target: String,
    private val endpoint: String,
    private val token: String?,
    private val gate: RequestGate,
) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        val joined = lines.joinToString("\n") { encodeCue(it) }
        val body = buildJsonObject {
            put("text", joined)
            put("source_lang", "auto")
            put("target_lang", SubtitleTranslators.deeplTarget(target))
        }
        val headers = token?.takeIf { it.isNotBlank() }?.let { mapOf("Authorization" to "Bearer $it") }.orEmpty()
        val res = client.postJson(endpoint, body, gate, headers)
        val data = (res as? JsonObject)?.get("data")?.jsonPrimitive?.contentOrNull
            ?: throw TranslationException(TranslationException.Kind.FORMAT, "DeepLX: no data")
        return (splitBack(lines, data) ?: if (lines.size == 1) listOf(data) else null)?.map(::decodeCue)
            ?: halves(lines)
    }
}

/** Azure Translator v3: `POST /translate?api-version=3.0&to=es` with `[{"Text":...}]` -> `[{"translations":[{"text"}]}]`. */
internal class AzureTranslator(
    private val client: OkHttpClient,
    private val target: String,
    private val endpoint: String,
    private val key: String,
    private val region: String,
    private val gate: RequestGate,
) : SubtitleTranslator {
    override suspend fun translate(lines: List<String>): List<String> {
        if (lines.isEmpty()) return emptyList()
        val url = "$endpoint/translate".toHttpUrlOrNull()?.newBuilder()
            ?.addQueryParameter("api-version", "3.0")?.addQueryParameter("to", SubtitleTranslators.azureCode(target))?.build()
            ?: throw TranslationException(TranslationException.Kind.MISSING_ENDPOINT, "bad endpoint")
        val body = buildJsonArray { lines.forEach { line -> addJsonObject { put("Text", line) } } }
        val res = client.postJson(url.toString(), body, gate, mapOf("Ocp-Apim-Subscription-Key" to key, "Ocp-Apim-Subscription-Region" to region))
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
    private val gate: RequestGate,
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
        val res = client.postJson(endpoint, body, gate, headers)
        val choice = (res as? JsonObject)?.get("choices")?.jsonArray?.firstOrNull()?.jsonObject
        val content = choice?.get("message")?.jsonObject?.get("content")?.jsonPrimitive?.contentOrNull
            ?: throw TranslationException(TranslationException.Kind.FORMAT, "OpenAI: no content")
        if (choice["finish_reason"]?.jsonPrimitive?.contentOrNull == "length") {
            if (lines.size > 1) return halves(lines)
            throw TranslationException(TranslationException.Kind.FORMAT, "OpenAI: truncated")
        }
        val cleaned = content.replace(Regex("^(\\s*<think>[\\s\\S]*?</think>)+\\s*", RegexOption.IGNORE_CASE), "").trim()
        return parseNumbered(cleaned, lines.size)?.map(::decodeCue)
            ?: if (lines.size == 1) listOf(decodeCue(cleaned.removePrefix("1.").trim())) else halves(lines)
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
 * Translation cache (player.subs.translate.cache): memory + one JSON file per scope under `cacheDir/subs-translate/`
 * (`original -> translated`). Re-watching never calls the provider again; "Vaciar caché" deletes the folder. Thread-safe.
 *
 * 0.4.0: the scope is the series (not the episode) so openings, endings and recurring lines are translated once per series; the
 * file is rewritten at most every [WRITE_INTERVAL_MS] (look-ahead puts a batch every few seconds) and on [flush].
 */
class TranslationCache(private val dir: File, private val clock: MonotonicClock = MonotonicClock.SYSTEM) {
    private val mutex = Mutex()
    private val memory = HashMap<String, MutableMap<String, String>>()
    private val dirty = HashSet<String>()
    private var lastWriteMs = Long.MIN_VALUE

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
        while (t.size > MAX_ENTRIES_PER_SCOPE) t.remove(t.keys.first())
        dirty += scope
        val now = clock.nowMs()
        if (lastWriteMs == Long.MIN_VALUE || now - lastWriteMs >= WRITE_INTERVAL_MS) writeDirty(now)
    }

    /** Writes every scope changed since the last write (the player calls it when a pre-translation ends / the session changes). */
    suspend fun flush() = mutex.withLock { writeDirty(clock.nowMs()) }

    private suspend fun writeDirty(now: Long) {
        if (dirty.isEmpty()) return
        val scopes = dirty.toList()
        dirty.clear()
        lastWriteMs = now
        withContext(Dispatchers.IO) {
            dir.mkdirs()
            for (scope in scopes) {
                val t = memory[scope] ?: continue
                val json = buildJsonObject { t.forEach { (k, v) -> put(k, v) } }
                val tmp = File(dir, fileOf(scope).name + ".tmp")
                tmp.writeText(json.toString())
                if (!tmp.renameTo(fileOf(scope))) {
                    fileOf(scope).writeText(json.toString())
                    tmp.delete()
                }
            }
        }
    }

    /** "Vaciar caché". */
    suspend fun clear() = mutex.withLock {
        memory.clear()
        dirty.clear()
        withContext(Dispatchers.IO) { dir.listFiles()?.forEach { it.delete() } }
    }

    /** Bytes on disk (shown next to "Vaciar caché"). */
    fun sizeBytes(): Long = dir.listFiles()?.sumOf { it.length() } ?: 0L

    companion object {
        /** A whole series (24 episodes x ~400 distinct lines) fits; the oldest entries go first beyond it. */
        const val MAX_ENTRIES_PER_SCOPE = 20_000
        const val WRITE_INTERVAL_MS = 4_000L

        /** Cache scope of one series ([seriesKey]) in one target language with one provider. */
        fun scope(seriesKey: String, config: TranslatorConfig): String = "${config.provider.id}|${config.targetLang}|$seriesKey"

        private fun sha1(s: String): String =
            MessageDigest.getInstance("SHA-1").digest(s.toByteArray()).joinToString("") { "%02x".format(it) }
    }
}

/**
 * Live + look-ahead + preload translation over one [SubtitleTranslator] and the [TranslationCache] of one series.
 *
 * - Live ([translateNow]): the lines on screen that are not cached yet, plus (0.4.0) the next cues of the [setTimeline] track, in ONE
 *   request of at most [BATCH_MAX_LINES] lines / [BATCH_MAX_CHARS] characters.
 * - Look-ahead ([translateAhead], 0.4.0): when the full cue list of the track is known (sidecar, loaded file or the embedded track read
 *   by [EmbeddedSubtitleExtractor]), the next untranslated cue is requested [LOOKAHEAD_TRIGGER_MS] before it shows, together with the
 *   ones after it, so lines are ready before they appear and a whole episode costs ~10 requests instead of one per line.
 * - Preload ([pretranslate]): the whole track in batches with progress; a rate-limited batch waits for the provider and is retried.
 */
class SubtitleTranslationSession(
    private val translator: SubtitleTranslator,
    private val cache: TranslationCache,
    private val scope: String,
) {
    @Volatile
    private var timeline: List<TimedCue> = emptyList()

    @Volatile
    private var skip: Set<String> = emptySet()

    /** Full (or partial, growing) cue list of the selected track, normalized; [skipTexts] = karaoke / drawings never translated. */
    fun setTimeline(cues: List<TimedCue>?, skipTexts: Set<String> = emptySet()) {
        timeline = cues.orEmpty()
        skip = skipTexts
    }

    fun hasTimeline(): Boolean = timeline.isNotEmpty()

    /** Kept for the settings "Probar" path and older callers: [lines] in, one output per line (original when not translatable). */
    suspend fun translateLive(lines: List<String>): List<String> {
        val map = translateNow(lines, null)
        return lines.map { map[it] ?: it }
    }

    /**
     * The lines on screen ([live], already normalized) plus look-ahead from [positionMs]: one request at most. Returns every translation
     * now known for [live] (cached or fresh) and the fresh look-ahead ones. Throws RATE_LIMITED / HTTP errors to the caller.
     */
    suspend fun translateNow(live: List<String>, positionMs: Long?): Map<String, String> {
        val wanted = live.filter { it.isNotBlank() && it !in skip }.distinct()
        val known = HashMap<String, String>()
        if (wanted.isNotEmpty()) {
            val cached = cache.get(scope, wanted)
            wanted.forEachIndexed { i, l -> cached[i]?.let { known[l] = it } }
        }
        val missing = wanted.filter { it !in known }
        if (missing.isEmpty()) return known
        val ahead = if (positionMs != null) upcomingUncached(positionMs) else emptyList()
        val batch = fillBatch(missing, ahead)
        return known + translateBatch(batch)
    }

    /** Look-ahead only: one batch when the next untranslated cue starts within [LOOKAHEAD_TRIGGER_MS]; empty when nothing is due. */
    suspend fun translateAhead(positionMs: Long): Map<String, String> {
        val ahead = upcomingUncached(positionMs)
        val first = ahead.firstOrNull() ?: return emptyMap()
        if (first.startMs > positionMs + LOOKAHEAD_TRIGGER_MS) return emptyMap()
        return translateBatch(fillBatch(emptyList(), ahead))
    }

    /** Upcoming cues (visible now or starting within [LOOKAHEAD_MAX_MS]) whose text is neither cached nor skipped, in time order. */
    private suspend fun upcomingUncached(positionMs: Long): List<TimedCue> {
        val candidates = upcoming(timeline, positionMs, LOOKAHEAD_MAX_MS, MAX_CANDIDATES).filter { it.text !in skip }
        if (candidates.isEmpty()) return emptyList()
        val texts = candidates.map { it.text }.distinct()
        val cached = cache.get(scope, texts)
        val done = texts.filterIndexed { i, _ -> cached[i] != null }.toHashSet()
        return candidates.filter { it.text !in done }
    }

    private suspend fun translateBatch(batch: List<String>): Map<String, String> {
        if (batch.isEmpty()) return emptyMap()
        val out = translator.translate(batch)
        if (out.size != batch.size) throw TranslationException(TranslationException.Kind.FORMAT, "count mismatch")
        val pairs = batch.zip(out).toMap()
        cache.put(scope, pairs)
        return pairs
    }

    /** Only cached lines, null for the others (no network): what the screen shows while a live request is in flight. */
    suspend fun cachedOnly(lines: List<String>): List<String?> = cache.get(scope, lines)

    suspend fun flush() = cache.flush()

    /**
     * Translates every text not cached yet; [onProgress] gets (done, total) after each batch. A rate-limited batch reports the wait to
     * [onThrottled], waits and is retried (at most [MAX_THROTTLE_WAIT_MS] in total; after that the rest is left to live translation).
     * Returns how many lines failed.
     */
    suspend fun pretranslate(texts: List<String>, onThrottled: (waitMs: Long) -> Unit = {}, onProgress: (done: Int, total: Int) -> Unit): Int {
        val unique = texts.filter { it.isNotBlank() && it !in skip }.distinct()
        val cached = cache.get(scope, unique)
        val todo = unique.filterIndexed { i, _ -> cached[i] == null }
        var done = unique.size - todo.size
        var failed = 0
        var waited = 0L
        onProgress(done, unique.size)
        val queue = ArrayDeque(batches(todo))
        while (queue.isNotEmpty()) {
            val batch = queue.removeFirst()
            try {
                translateBatch(batch)
            } catch (e: CancellationException) {
                throw e
            } catch (e: TranslationException) {
                val wait = e.retryAfterMs
                if (e.kind == TranslationException.Kind.RATE_LIMITED && wait != null && waited + wait <= MAX_THROTTLE_WAIT_MS) {
                    onThrottled(wait)
                    delay(wait)
                    waited += wait
                    queue.addFirst(batch)
                    continue
                }
                failed += batch.size
            } catch (e: Exception) {
                failed += batch.size
            }
            done += batch.size
            onProgress(done, unique.size)
        }
        cache.flush()
        return failed
    }

    companion object {
        const val BATCH_MAX_LINES = 40
        const val BATCH_MAX_CHARS = 3000

        /** Request the next untranslated cue this long before it shows (one paced Google request takes < 2 s). */
        const val LOOKAHEAD_TRIGGER_MS = 90_000L

        /** Never fill a batch with cues further away than this (the viewer may stop watching). */
        const val LOOKAHEAD_MAX_MS = 8 * 60_000L
        const val MAX_THROTTLE_WAIT_MS = 3 * 60_000L
        private const val MAX_CANDIDATES = 200

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

        /** Cues still to come at [positionMs] (visible now or starting within [windowMs]), in time order, at most [max]. */
        fun upcoming(cues: List<TimedCue>, positionMs: Long, windowMs: Long, max: Int = Int.MAX_VALUE): List<TimedCue> {
            if (cues.isEmpty()) return emptyList()
            // Cues are sorted by start; overlapping long cues (signs) that started earlier are picked up by the short walk back.
            var lo = 0
            var hi = cues.size
            while (lo < hi) {
                val mid = (lo + hi) ushr 1
                if (cues[mid].startMs < positionMs) lo = mid + 1 else hi = mid
            }
            val out = ArrayList<TimedCue>()
            var back = lo - 1
            var scanned = 0
            while (back >= 0 && scanned < 16) {
                if (cues[back].endMs > positionMs) out.add(0, cues[back])
                back--
                scanned++
            }
            val limit = positionMs + windowMs
            var i = lo
            while (i < cues.size && cues[i].startMs <= limit && out.size < max) {
                out += cues[i]
                i++
            }
            return out
        }

        /** [live] lines first (they are on screen), then [ahead] texts in time order, within the batch caps; distinct. */
        fun fillBatch(live: List<String>, ahead: List<TimedCue>): List<String> {
            val out = LinkedHashSet<String>()
            var chars = 0
            for (l in live) {
                // Live lines always go, even past the caps: the viewer is waiting for them.
                if (out.add(l)) chars += l.length
            }
            for (c in ahead) {
                if (out.size >= BATCH_MAX_LINES || chars + c.text.length > BATCH_MAX_CHARS) break
                if (out.add(c.text)) chars += c.text.length
            }
            return out.toList()
        }
    }
}
