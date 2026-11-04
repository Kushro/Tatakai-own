// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtLogLevel
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import java.util.concurrent.ConcurrentHashMap

/**
 * Session side of the browser runtime: what `__host_browser(op, id, arg)` runs for ONE extension runtime (port of desktop
 * `tk-ext/src/runtime.rs::browser_op`, docs/redesign/12 §3.7). The legacy checks are enforced here and repeated by the [BrowserRuntime]
 * implementation: an id is only usable by the runtime that opened it, at most [BrowserPolicy.MAX_BROWSERS_PER_EXTENSION] are open, every
 * URL passes [BrowserPolicy.urlAllowed], expressions and results are size-capped, each operation is bounded by the browser's
 * `clamp(timeout, 1, 30)` s. [call] never throws (except cancellation): it answers the JSON envelope `{"ok":true,"value":...}` /
 * `{"ok":false,"error":"<BrowserError message>"}` that the prelude turns into a resolved / rejected promise.
 */
class BrowserBridge(
    private val runtime: BrowserRuntime,
    private val extId: String,
    /** [BrowserPolicy.browserDomains] of the manifest; never empty when the bridge is installed. */
    val domains: List<String>,
    private val onEvent: (level: ExtLogLevel, message: String) -> Unit = { _, _ -> },
) {
    /** Open browser ids of this runtime -> per-operation timeout (ms). */
    private val owned = ConcurrentHashMap<String, Long>()

    /** Set when a `new` operation starts; the engine resets it per call to grant [BrowserPolicy.CALL_BUDGET_WITH_BROWSER_MS]. */
    @Volatile var openedBrowser: Boolean = false

    val openCount: Int get() = owned.size

    suspend fun call(op: String, id: String, arg: String): String {
        val result: Result<JsonElement> = try {
            Result.success(perform(op, id, arg))
        } catch (e: CancellationException) {
            throw e
        } catch (e: BrowserError) {
            Result.failure(e)
        } catch (e: Exception) {
            Result.failure(BrowserError.Failed(e.message?.takeIf { it.isNotBlank() } ?: "ChromeDP operation failed"))
        }
        return result.fold(
            onSuccess = { v -> buildJsonObject { put("ok", true); put("value", v) }.toString() },
            onFailure = { e ->
                if (e is BrowserError.Closed) id.takeIf { it.isNotEmpty() }?.let { owned.remove(it) } // auto-closed (150 s) or gone
                buildJsonObject { put("ok", false); put("error", e.message ?: "ChromeDP operation failed") }.toString()
            },
        )
    }

    private suspend fun perform(op: String, id: String, arg: String): JsonElement {
        if (op == "new") {
            openedBrowser = true
            if (domains.isEmpty()) throw BrowserError.NoDomains()
            if (owned.size >= BrowserPolicy.MAX_BROWSERS_PER_EXTENSION) throw BrowserError.Limit()
            val opts = runCatching { Json.parseToJsonElement(arg) }.getOrNull() as? JsonObject ?: JsonObject(emptyMap())
            val timeout = browserOperationTimeoutMs(opts)
            if (browserWantsWindow(opts)) onEvent(ExtLogLevel.WARN, "headless: false is ignored on Android; the browser stays hidden")
            val created = runtime.newBrowser(extId, domains, opts)
            owned[created] = timeout
            onEvent(ExtLogLevel.OK, "browser opened (${domains.size} domains, timeout ${timeout / 1000} s)")
            return JsonPrimitive(created)
        }
        val timeout = owned[id] ?: throw BrowserError.Closed()
        return when (op) {
            "close" -> {
                owned.remove(id)
                runtime.close(id)
                JsonNull
            }
            "navigate" -> {
                if (!BrowserPolicy.urlAllowed(arg, domains)) {
                    onEvent(ExtLogLevel.WARN, "navigation outside the provider website was refused")
                    throw BrowserError.UrlNotAllowed(arg)
                }
                bounded(timeout) { runtime.navigate(id, arg) }
                JsonNull
            }
            "evaluate" -> {
                if (arg.toByteArray(Charsets.UTF_8).size > BrowserPolicy.MAX_EXPRESSION_BYTES) throw BrowserError.Failed("Invalid ChromeDP expression")
                capped(bounded(timeout) { runtime.evaluate(id, arg) })
            }
            "cookies" -> {
                val urls = (runCatching { Json.parseToJsonElement(arg) }.getOrNull() as? JsonArray)
                    ?.mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content }.orEmpty()
                if (urls.isEmpty() || urls.size > BrowserPolicy.MAX_COOKIE_URLS) throw BrowserError.Failed("ChromeDP cookie URLs are required")
                urls.firstOrNull { !BrowserPolicy.urlAllowed(it, domains) }?.let { throw BrowserError.UrlNotAllowed(it) }
                val cookies = bounded(timeout) { runtime.cookies(id, urls) }
                capped(buildJsonObject { put("cookies", JsonArray(cookies)) })
            }
            else -> throw BrowserError.Failed("Unsupported ChromeDP browser method: $op")
        }
    }

    /** Close every browser this runtime still owns (runtime discarded / closed). */
    suspend fun closeAll() {
        val ids = owned.keys.toList()
        owned.clear()
        for (id in ids) runCatching { runtime.close(id) }
    }

    /** Safety net over the runtime's own per-operation timeout. */
    private suspend fun <T> bounded(timeoutMs: Long, block: suspend () -> T): T =
        withTimeoutOrNull(timeoutMs + 2_000) { block() } ?: throw BrowserError.Timeout()

    private fun capped(v: JsonElement): JsonElement {
        if (v.toString().toByteArray(Charsets.UTF_8).size > BrowserPolicy.MAX_RESULT_BYTES) throw BrowserError.TooLarge()
        return v
    }
}
