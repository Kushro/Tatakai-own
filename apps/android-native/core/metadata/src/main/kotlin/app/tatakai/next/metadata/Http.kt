package app.tatakai.next.metadata

import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import okhttp3.Call
import okhttp3.Callback
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import java.io.IOException
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

internal val MetadataJson = Json { ignoreUnknownKeys = true; explicitNulls = true }

/** Executes [request] and returns the body text; maps transport errors to status 0 and non-2xx to the HTTP status. */
internal suspend fun OkHttpClient.fetchText(provider: String, request: Request): String {
    val response = awaitResponse(provider, request)
    response.use { r ->
        val body = try {
            r.body.string()
        } catch (e: IOException) {
            throw ProviderException(provider, 0, "read failed: ${e.message}", e)
        }
        if (!r.isSuccessful) throw ProviderException(provider, r.code, "HTTP ${r.code}")
        return body
    }
}

internal suspend fun OkHttpClient.fetchJson(provider: String, request: Request): JsonElement {
    val text = fetchText(provider, request)
    return try {
        MetadataJson.parseToJsonElement(text)
    } catch (e: Exception) {
        throw ProviderException(provider, 502, "invalid JSON", e)
    }
}

private suspend fun OkHttpClient.awaitResponse(provider: String, request: Request): Response =
    suspendCancellableCoroutine { cont ->
        val call = newCall(request)
        cont.invokeOnCancellation { call.cancel() }
        call.enqueue(object : Callback {
            override fun onFailure(call: Call, e: IOException) {
                if (cont.isActive) cont.resumeWithException(ProviderException(provider, transportStatus(e), e.message ?: "network error", e))
            }

            override fun onResponse(call: Call, response: Response) {
                if (cont.isActive) cont.resume(response) else response.close()
            }
        })
    }

private val CONNECT_STATUS = Regex("""Unexpected response code for CONNECT: (\d{3})""")

/**
 * BUG-146: status of a transport failure. 0 (offline) except when an HTTPS proxy refused the tunnel with an HTTP status (OkHttp's
 * "Unexpected response code for CONNECT: 429"): that one is the server's answer, so a 429 there is a rate limit, not "Sin conexión".
 */
internal fun transportStatus(e: IOException): Int =
    e.message?.let { CONNECT_STATUS.find(it) }?.groupValues?.get(1)?.toIntOrNull() ?: 0
