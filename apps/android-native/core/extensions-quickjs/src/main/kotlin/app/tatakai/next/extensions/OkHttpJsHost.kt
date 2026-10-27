// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import okhttp3.Dns
import okhttp3.Headers.Companion.toHeaders
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import java.io.IOException
import java.net.InetAddress
import java.net.UnknownHostException
import java.util.concurrent.TimeUnit

/**
 * [JsHost] for one extension: `fetch` gated by the manifest's `network:domain:*` permissions (initial host) and by the SSRF policy on
 * EVERY hop and resolved address (DNS is resolved here, so a rebinding answer is validated at connect time). No cookie jar,
 * max 5 redirects (followed manually; Cookie/Authorization dropped on origin change), 30 s call timeout, 25 MiB response cap.
 * No Supabase session or app credential is ever added to a request.
 */
class OkHttpJsHost(
    override val networkPolicy: NetworkPolicy,
    base: OkHttpClient,
    private val onLog: (level: String, message: String) -> Unit = { _, _ -> },
) : JsHost {
    private val client: OkHttpClient = base.newBuilder()
        .followRedirects(false)
        .followSslRedirects(false)
        .cache(null)
        .cookieJar(okhttp3.CookieJar.NO_COOKIES)
        .dns(SsrfDns)
        .callTimeout(30, TimeUnit.SECONDS)
        .build()

    override suspend fun fetch(request: HostFetchRequest): HostFetchResponse = withContext(Dispatchers.IO) {
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
            val response = try {
                call.execute()
            } catch (e: SsrfBlockedException) {
                throw ExtError.PermissionDenied()
            } catch (e: IOException) {
                if (e.cause is SsrfBlockedException) throw ExtError.PermissionDenied()
                throw ExtError.BundleError("Network request failed: ${e.javaClass.simpleName}")
            }
            response.use { r ->
                val location = r.header("Location")
                if (r.code in REDIRECTS && location != null) {
                    if (++hops > MAX_REDIRECTS) throw ExtError.BundleError("Too many redirects")
                    val next = httpUrl.resolve(location) ?: throw ExtError.PermissionDenied()
                    if (next.scheme != "http" && next.scheme != "https") throw ExtError.PermissionDenied()
                    if (next.host != httpUrl.host || next.port != httpUrl.port || next.scheme != httpUrl.scheme) {
                        headers = headers.filterKeys { it.lowercase() != "cookie" && it.lowercase() != "authorization" }
                    }
                    if (r.code == 303 || ((r.code == 301 || r.code == 302) && method == "POST")) {
                        method = "GET"; body = null
                    }
                    url = next.toString()
                    return@use
                }
                val source = r.body.source()
                if (r.body.contentLength() > MAX_RESPONSE_BYTES || source.request(MAX_RESPONSE_BYTES + 1)) {
                    throw ExtError.BundleError("Response larger than 25 MiB")
                }
                val bytes = source.buffer.readByteArray()
                val out = LinkedHashMap<String, String>()
                for (name in r.headers.names()) out[name.lowercase()] = r.headers.values(name).joinToString(", ")
                return@withContext HostFetchResponse(url, r.code, r.message, out, bytes)
            }
        }
        @Suppress("UNREACHABLE_CODE")
        error("unreachable")
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
        private val IP_LITERAL = Regex("""\d{1,3}(\.\d{1,3}){3}""")
        private val REDIRECTS = setOf(301, 302, 303, 307, 308)
        private val FORBIDDEN_HEADERS = setOf("host", "content-length", "connection", "transfer-encoding")
    }
}
