// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtLogLevel
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.suspendCancellableCoroutine
import okhttp3.Call
import okhttp3.Callback
import okhttp3.Dispatcher
import okhttp3.Dns
import okhttp3.Headers.Companion.toHeaders
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import java.io.IOException
import java.net.InetAddress
import java.net.UnknownHostException
import java.util.concurrent.TimeUnit
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * [JsHost] for one extension: `fetch` gated by the manifest's `network:domain:*` permissions (initial host) and by the SSRF policy on
 * EVERY hop and resolved address (DNS is resolved here, so a rebinding answer is validated at connect time). No cookie jar,
 * max 5 redirects (followed manually; Cookie/Authorization dropped on origin change), 30 s call timeout, 25 MiB response cap.
 * No OAuth token or app credential is ever added to a request.
 */
class OkHttpJsHost(
    override val networkPolicy: NetworkPolicy,
    base: OkHttpClient,
    private val onLog: (level: String, message: String) -> Unit = { _, _ -> },
    /** android wave (12 §4 e): per-extension log lines (fetch refusals/failures, browser events). */
    private val onEvent: (level: ExtLogLevel, op: String, message: String) -> Unit = { _, _, _ -> },
    /** BUG-128: debug-only lines (logcat), e.g. a request dropped because its call already answered. */
    private val onDebug: (message: String) -> Unit = {},
) : JsHost {
    private val client: OkHttpClient = base.newBuilder()
        .followRedirects(false)
        .followSslRedirects(false)
        .cache(null)
        .cookieJar(okhttp3.CookieJar.NO_COOKIES)
        .dns(SsrfDns)
        .callTimeout(30, TimeUnit.SECONDS)
        // Requests are enqueued (cancellable, BUG-128); bundles fan out to many hosts at once, as `execute()` allowed.
        .dispatcher(DISPATCHER)
        .build()

    override suspend fun fetch(request: HostFetchRequest): HostFetchResponse =
        try {
            doFetch(request)
        } catch (e: CancellationException) {
            // BUG-128: the call this request belonged to already answered (or was cancelled): nobody reads the result, so it is
            // not an error of the extension; the OkHttp call itself was cancelled.
            onDebug("request to ${hostOf(request.url)} dropped: its call already answered")
            throw e
        } catch (e: ExtError.PermissionDenied) {
            onEvent(ExtLogLevel.WARN, "fetch", "request to ${hostOf(request.url)} refused: outside the manifest permissions or a local address")
            throw e
        } catch (e: ExtError) {
            onEvent(ExtLogLevel.ERR, "fetch", "request to ${hostOf(request.url)} failed: ${e.message}")
            throw e
        }

    override fun event(level: ExtLogLevel, op: String, message: String) = onEvent(level, op, message)

    private fun hostOf(url: String): String = url.toHttpUrlOrNull()?.host ?: "an invalid URL"

    /** One hop: the redirect target, or the final response. */
    private sealed class Hop {
        class Redirect(val code: Int, val location: String) : Hop()
        class Done(val response: HostFetchResponse) : Hop()
    }

    private suspend fun doFetch(request: HostFetchRequest): HostFetchResponse {
        networkPolicy.requireAllowed(request.url)
        var url = request.url
        var method = request.method.uppercase()
        var body = request.body
        var headers = request.headers.filterKeys { it.lowercase() !in FORBIDDEN_HEADERS }
        var hops = 0
        while (true) {
            val httpUrl = url.toHttpUrlOrNull() ?: throw ExtError.PermissionDenied()
            if (SsrfGuard.isForbiddenHostName(httpUrl.host)) throw ExtError.PermissionDenied()
            // OkHttp does not consult Dns for IP literals: validate them here.
            if (IP_LITERAL.matches(httpUrl.host) || httpUrl.host.contains(':')) {
                val literal = try { InetAddress.getByName(httpUrl.host) } catch (_: Exception) { throw ExtError.PermissionDenied() }
                if (SsrfGuard.isForbidden(literal)) throw ExtError.PermissionDenied()
            }
            val builder = Request.Builder().url(httpUrl).headers(headers.toHeaders())
            val reqBody = if (method == "GET" || method == "HEAD") null else (body ?: ByteArray(0)).toRequestBody()
            val call = client.newCall(builder.method(method, reqBody).build())
            val hopUrl = url
            when (val hop = exchange(call) { r -> readHop(r, hopUrl) }) {
                is Hop.Done -> return hop.response
                is Hop.Redirect -> {
                    if (++hops > MAX_REDIRECTS) throw ExtError.BundleError("Too many redirects")
                    val next = httpUrl.resolve(hop.location) ?: throw ExtError.PermissionDenied()
                    if (next.scheme != "http" && next.scheme != "https") throw ExtError.PermissionDenied()
                    if (next.host != httpUrl.host || next.port != httpUrl.port || next.scheme != httpUrl.scheme) {
                        headers = headers.filterKeys { it.lowercase() != "cookie" && it.lowercase() != "authorization" }
                    }
                    if (hop.code == 303 || ((hop.code == 301 || hop.code == 302) && method == "POST")) {
                        method = "GET"; body = null
                    }
                    url = next.toString()
                }
            }
        }
    }

    private fun readHop(r: Response, url: String): Hop {
        val location = r.header("Location")
        if (r.code in REDIRECTS && location != null) return Hop.Redirect(r.code, location)
        val source = r.body.source()
        if (r.body.contentLength() > MAX_RESPONSE_BYTES || source.request(MAX_RESPONSE_BYTES + 1)) {
            throw ExtError.BundleError("Response larger than 25 MiB")
        }
        val bytes = source.buffer.readByteArray()
        val out = LinkedHashMap<String, String>()
        for (name in r.headers.names()) out[name.lowercase()] = r.headers.values(name).joinToString(", ")
        return Hop.Done(HostFetchResponse(url, r.code, r.message, out, bytes))
    }

    /**
     * BUG-128: runs [call] on OkHttp's dispatcher and reads the response there; cancelling the coroutine cancels the call (also
     * mid-body). A blocking `execute()` kept going up to its 30 s timeout after the extension call had answered, and logged its
     * failure as an error of the extension.
     */
    private suspend fun <T> exchange(call: Call, read: (Response) -> T): T = suspendCancellableCoroutine { cont ->
        cont.invokeOnCancellation { call.cancel() }
        call.enqueue(object : Callback {
            override fun onFailure(call: Call, e: IOException) {
                cont.resumeWithException(mapIo(e))
            }

            override fun onResponse(call: Call, response: Response) {
                val result = try {
                    response.use { Result.success(read(it)) }
                } catch (e: IOException) {
                    Result.failure(mapIo(e))
                } catch (e: Exception) {
                    Result.failure(e)
                }
                result.fold({ cont.resume(it) }, { cont.resumeWithException(it) })
            }
        })
    }

    private fun mapIo(e: IOException): Exception = when {
        e is SsrfBlockedException || e.cause is SsrfBlockedException -> ExtError.PermissionDenied()
        else -> ExtError.BundleError("Network request failed: ${e.javaClass.simpleName}")
    }

    override fun log(level: String, message: String) = onLog(level, message.take(MAX_LOG_CHARS))

    private class SsrfBlockedException : IOException("blocked")

    private object SsrfDns : Dns {
        override fun lookup(hostname: String): List<InetAddress> {
            val all = try {
                Dns.SYSTEM.lookup(hostname)
            } catch (e: UnknownHostException) {
                throw e
            }
            if (all.isEmpty() || all.any { SsrfGuard.isForbidden(it) }) throw SsrfBlockedException()
            return all
        }
    }

    companion object {
        const val MAX_RESPONSE_BYTES = 25L * 1024 * 1024
        const val MAX_REDIRECTS = 5
        const val MAX_LOG_CHARS = 4096
        /** Shared by every extension host; the defaults (64 requests, 5 per host) would queue a bundle's provider fan-out. */
        private val DISPATCHER = Dispatcher().apply {
            maxRequests = 256
            maxRequestsPerHost = 32
        }
        private val IP_LITERAL = Regex("""\d{1,3}(\.\d{1,3}){3}""")
        private val REDIRECTS = setOf(301, 302, 303, 307, 308)
        private val FORBIDDEN_HEADERS = setOf("host", "content-length", "connection", "transfer-encoding")
    }
}
