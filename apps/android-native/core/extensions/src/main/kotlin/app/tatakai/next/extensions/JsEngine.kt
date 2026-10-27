// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.serialization.json.JsonElement

/** What a [JsEngine] needs from the app. All side effects of a bundle go through this (no ambient authority). */
interface JsHost {
    val networkPolicy: NetworkPolicy

    /**
     * Permission-gated and SSRF-guarded fetch (globals.v1.md section 2): at most 5 redirects, 30 s timeout,
     * 25 MiB response cap, no cookie persistence.
     * @throws ExtError.PermissionDenied when the host is outside the manifest permissions or is a forbidden address.
     */
    suspend fun fetch(request: HostFetchRequest): HostFetchResponse

    /** `console.*` of the bundle. Payloads over 4 KiB must be truncated by the implementation. */
    fun log(level: String, message: String)
}

class HostFetchRequest(
    val url: String,
    val method: String = "GET",
    val headers: Map<String, String> = emptyMap(),
    val body: ByteArray? = null,
)

class HostFetchResponse(
    val url: String,
    val status: Int,
    val statusText: String,
    val headers: Map<String, String>,
    val body: ByteArray,
)

/** Result of one bundle method call: the returned value plus anything handed to the `onChunk` callback. */
class JsInvocation(val result: JsonElement, val chunks: List<JsonElement> = emptyList())

/** One live QuickJS runtime holding an evaluated bundle instance. Calls are serialized by the caller. */
interface JsSession : AutoCloseable {
    suspend fun hasMethod(name: String): Boolean

    /**
     * Calls `instance[method](...args)`. With [collectChunks] an extra `onChunk(value)` function is appended to the arguments
     * and its values are returned in [JsInvocation.chunks]. Enforces the 15 s budget and 64 MiB limit.
     * @throws ExtError.Timeout, ExtError.OutOfMemory (the session must then be closed), ExtError.BundleError, ExtError.PermissionDenied
     */
    suspend fun invoke(method: String, args: List<JsonElement>, collectChunks: Boolean = false): JsInvocation

    override fun close()
}

/** Creates sessions. The Android implementation is `QuickJsEngine` in `:core:extensions-quickjs`. */
interface JsEngine {
    /** Evaluates [bundle] (CommonJS) and instantiates the export. @throws ExtError.BundleError when it produces no usable export. */
    suspend fun open(manifest: ExtensionManifest, bundle: String, host: JsHost): JsSession

    companion object {
        const val CALL_TIMEOUT_MS = 15_000L
        const val MEMORY_LIMIT_BYTES = 64L * 1024 * 1024
    }
}
