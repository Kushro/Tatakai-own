// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import com.dokar.quickjs.QuickJs
import com.dokar.quickjs.QuickJsException
import com.dokar.quickjs.QuickJsInterruptedException
import com.dokar.quickjs.binding.asyncFunction
import com.dokar.quickjs.binding.function
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withTimeout
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.put
import org.jsoup.Jsoup
import org.jsoup.nodes.Element
import java.security.MessageDigest
import java.security.SecureRandom
import java.util.Base64
import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * `quickjs-kt` 1.0.15 implementation of [JsEngine]: ONE runtime per extension, 64 MiB memory limit, 15 s per host call, the globals of
 * contracts/extension-host/globals.v1.md provided by `tatakai/host-prelude.js` on top of a handful of `__host_*` bridge functions.
 * Everything crossing the bridge is a JSON string, so no QuickJS object marshalling is relied upon.
 */
class QuickJsEngine(
    private val dispatcher: CoroutineDispatcher = Dispatchers.Default,
) : JsEngine {
    private val prelude: String by lazy {
        QuickJsEngine::class.java.getResourceAsStream("/tatakai/host-prelude.js")?.use { it.readBytes().toString(Charsets.UTF_8) }
            ?: error("host-prelude.js missing from resources")
    }

    override suspend fun open(manifest: ExtensionManifest, bundle: String, host: JsHost): JsSession {
        val qjs = QuickJs.create(dispatcher)
        val session = QuickJsSession(qjs, host)
        try {
            qjs.memoryLimit = JsEngine.MEMORY_LIMIT_BYTES
            qjs.maxStackSize = MAX_STACK_BYTES
            qjs.evaluationTimeoutMillis = JsEngine.CALL_TIMEOUT_MS
            session.bind()
            withTimeout(JsEngine.CALL_TIMEOUT_MS) {
                qjs.evaluate<Any?>(prelude, "host-prelude.js")
                // Same parameter order as the legacy `new Function` (globals.v1.md section 1). Header and bundle share line 1.
                qjs.evaluate<Any?>(
                    "globalThis.__tk_bundle_fn = (function(module, exports, require, __tatakai_fetch__, __tatakai_anitomy__, " +
                        "__tatakai_parse_html__, Buffer, process, global, setImmediate, clearImmediate, fetch){" + bundle + "\n});",
                    "bundle.js",
                )
                qjs.evaluate<Any?>("__tk_instantiate()", "instantiate.js")
            }
        } catch (e: Throwable) {
            runCatching { session.close() }
            if (e is CancellationException && e !is TimeoutCancellationException) throw e
            throw when (e) {
                is TimeoutCancellationException -> ExtError.Timeout()
                is ExtError -> e
                else -> session.mapFailure(e, "Extension failed to load")
            }
        }
        return session
    }

    companion object {
        const val MAX_STACK_BYTES = 1024L * 1024
    }
}

internal class QuickJsSession(private val qjs: QuickJs, private val host: JsHost) : JsSession {
    private val mutex = Mutex()
    private val html = HtmlRegistry()
    private val random = SecureRandom()
    @Volatile private var pendingArgs: String = "[]"

    fun bind() {
        qjs.function("__host_log") { a -> host.log(a[0].toString(), a[1].toString()); null }
        qjs.function("__host_args") { _ -> pendingArgs }
        qjs.asyncFunction("__host_sleep") { a -> delay(((a[0] as Number).toLong()).coerceIn(0, 15_000)); null }
        qjs.asyncFunction("__host_fetch") { a -> fetchJson(a[0].toString()) }
        qjs.function("__host_hash") { a -> b64(digest(a[0].toString()).digest(unb64(a[1].toString()))) }
        qjs.function("__host_hmac") { a ->
            val algo = macName(a[0].toString())
            val mac = Mac.getInstance(algo)
            val key = unb64(a[1].toString())
            mac.init(SecretKeySpec(if (key.isEmpty()) ByteArray(1) else key, algo))
            b64(mac.doFinal(unb64(a[2].toString())))
        }
        qjs.function("__host_random") { a ->
            val n = (a[0] as Number).toInt().coerceIn(0, 65536)
            b64(ByteArray(n).also { random.nextBytes(it) })
        }
        qjs.function("__host_html") { a -> html.op(a[0].toString(), (a[1] as Number).toInt(), a[2].toString()) }
    }

    private suspend fun fetchJson(requestJson: String): String {
        return try {
            val req = Json.parseToJsonElement(requestJson) as JsonObject
            val headers = (req["headers"] as? JsonObject)?.mapNotNull { (k, v) -> (v as? JsonPrimitive)?.contentOrNull?.let { k to it } }?.toMap().orEmpty()
            val body = (req["body"] as? JsonPrimitive)?.contentOrNull?.let(::unb64)
            val res = host.fetch(
                HostFetchRequest(
                    url = (req["url"] as JsonPrimitive).content,
                    method = (req["method"] as? JsonPrimitive)?.contentOrNull ?: "GET",
                    headers = headers,
                    body = body,
                ),
            )
            buildJsonObject {
                put("url", res.url); put("status", res.status); put("statusText", res.statusText)
                put("headers", JsonObject(res.headers.mapValues { JsonPrimitive(it.value) }))
                put("body", b64(res.body))
            }.toString()
        } catch (e: CancellationException) {
            throw e
        } catch (e: ExtError) {
            buildJsonObject { put("error", e.message ?: "Network error") }.toString()
        } catch (e: Exception) {
            buildJsonObject { put("error", "Network request failed") }.toString()
        }
    }

    override suspend fun hasMethod(name: String): Boolean {
        require(METHOD.matches(name)) { "bad method name" }
        return mutex.withLock { qjs.evaluate<Boolean>("__tk_has(\"$name\")") }
    }

    override suspend fun invoke(method: String, args: List<JsonElement>, collectChunks: Boolean): JsInvocation {
        require(METHOD.matches(method)) { "bad method name" }
        return mutex.withLock {
            pendingArgs = JsonArray(args).toString()
            html.clear()
            val text: String? = try {
                withTimeout(JsEngine.CALL_TIMEOUT_MS + 1_000) {
                    qjs.evaluate<String?>("await __tk_call(\"$method\", __host_args(), $collectChunks)", "call.js")
                }
            } catch (e: TimeoutCancellationException) {
                runCatching { qjs.interruptEvaluation() }
                throw ExtError.Timeout()
            } catch (e: QuickJsInterruptedException) {
                throw ExtError.Timeout()
            } catch (e: CancellationException) {
                throw e
            } catch (e: Exception) {
                throw mapFailure(e, "Extension call failed")
            }
            runCatching { qjs.evaluate<Any?>("__tk_reset_timers()", "reset.js") }
            val parsed = Json.parseToJsonElement(text ?: "{\"result\":null,\"chunks\":[]}") as JsonObject
            JsInvocation(parsed["result"] ?: JsonNull, (parsed["chunks"] as? JsonArray)?.toList().orEmpty())
        }
    }

    fun mapFailure(e: Throwable, fallback: String): ExtError {
        val msg = e.message.orEmpty()
        return when {
            e is ExtError -> e
            msg.contains("__TK_NOT_IMPLEMENTED__") -> ExtError.NotImplemented("Bundle does not implement ${msg.substringAfter("__TK_NOT_IMPLEMENTED__").takeWhile { it.isLetterOrDigit() || it == '_' }}")
            msg.contains("out of memory", ignoreCase = true) -> ExtError.OutOfMemory()
            msg.contains(ExtError.PermissionDenied.MESSAGE) -> ExtError.PermissionDenied()
            else -> ExtError.BundleError(msg.lineSequence().firstOrNull()?.take(500).takeUnless { it.isNullOrBlank() } ?: fallback, e)
        }
    }

    override fun close() {
        runCatching { qjs.close() }
    }

    private fun digest(algo: String): MessageDigest = MessageDigest.getInstance(
        when (algo) { "md5" -> "MD5"; "sha1" -> "SHA-1"; "sha256" -> "SHA-256"; "sha512" -> "SHA-512"; else -> throw IllegalArgumentException("digest") },
    )

    private fun macName(algo: String): String =
        when (algo) { "md5" -> "HmacMD5"; "sha1" -> "HmacSHA1"; "sha256" -> "HmacSHA256"; "sha512" -> "HmacSHA512"; else -> throw IllegalArgumentException("hmac") }

    private fun b64(bytes: ByteArray): String = Base64.getEncoder().encodeToString(bytes)

    private fun unb64(text: String): ByteArray = Base64.getDecoder().decode(text)

    companion object {
        private val METHOD = Regex("[A-Za-z_$][A-Za-z0-9_$]*")
    }
}

/** Handle table behind `__tatakai_parse_html__`: each handle is a static list of jsoup elements (cheerio-like, routes: globals.v1.md). */
internal class HtmlRegistry {
    private val handles = ArrayList<List<Element>>()

    fun clear() = handles.clear()

    private fun add(list: List<Element>): Int {
        check(handles.size < MAX_HANDLES) { "HTML handle space exhausted" }
        handles.add(list)
        return handles.size - 1
    }

    fun op(op: String, handle: Int, arg: String): String {
        if (op == "parse") {
            val doc = Jsoup.parse(arg)
            return add(listOf(doc.child(0))).toString()
        }
        val nodes = handles.getOrNull(handle) ?: emptyList()
        return when (op) {
            "len" -> nodes.size.toString()
            "eq" -> {
                val i = arg.toIntOrNull() ?: 0
                val idx = if (i < 0) nodes.size + i else i
                add(listOfNotNull(nodes.getOrNull(idx))).toString()
            }
            "parent" -> add(nodes.mapNotNull { it.parent() }.distinct()).toString()
            "children" -> add(nodes.flatMap { it.children() }.filter { arg.isEmpty() || it.`is`(arg) }).toString()
            "next" -> add(nodes.mapNotNull { it.nextElementSibling() }).toString()
            "prev" -> add(nodes.mapNotNull { it.previousElementSibling() }).toString()
            "find" -> add(LinkedHashSet<Element>().also { set -> nodes.forEach { set.addAll(it.select(arg)) } }.toList()).toString()
            "attr" -> nodes.firstOrNull()?.takeIf { it.hasAttr(arg) }?.attr(arg)?.let { JsonPrimitive(it).toString() } ?: "null"
            "text" -> JsonPrimitive(nodes.joinToString("") { it.wholeText() }).toString()
            "html" -> nodes.firstOrNull()?.let { JsonPrimitive(it.html()).toString() } ?: "null"
            else -> throw IllegalArgumentException("unknown html op")
        }
    }

    companion object {
        const val MAX_HANDLES = 200_000
    }
}
