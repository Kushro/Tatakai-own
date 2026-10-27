// SPDX-License-Identifier: AGPL-3.0-or-later
// Adapted from Enigmata (app.enigmata.temple.client.TempleHttp), same author, AGPL-3.0-or-later. See apps/android-native/THIRD_PARTY_NOTICES.md.
package app.tatakai.next.link

import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.Call
import okhttp3.Callback
import okhttp3.HttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody
import java.io.IOException
import java.io.InterruptedIOException
import java.security.cert.CertificateException
import java.util.Locale
import javax.net.ssl.SSLException
import javax.net.ssl.SSLHandshakeException
import javax.net.ssl.SSLPeerUnverifiedException
import kotlin.coroutines.resume

/**
 * Plumbing shared by [LinkClient] and [PairingClient]: the `Tatakai-Link` header, bounded body reads, the error table and the
 * transport classification. Nothing here logs; the server's error messages are parsed for their code only and dropped.
 */
internal object LinkHttp {
    const val CONTRACT_HEADER = "Tatakai-Link"
    const val CONTRACT = "v1"
    val JSON_TYPE = "application/json".toMediaType()
    private const val MAX_ERROR_BYTES = 16L * 1024
    private const val MAX_CAUSES = 8
    private const val ALERT_REVOKED = "certificate_revoked"
    private const val ALERT_EXPIRED = "certificate_expired"
    private const val ALERT = "alert"

    val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = true
        encodeDefaults = true
    }

    fun request(url: HttpUrl): Request.Builder = Request.Builder().url(url).header(CONTRACT_HEADER, CONTRACT)

    /**
     * Runs [request] without blocking the caller; cancelling the coroutine cancels the call. A 2xx answer goes to [onSuccess] with its
     * body (at most [maxBodyBytes]); [onSuccess] may throw [IllegalArgumentException] (decoding/validation) to report BODY.
     */
    suspend fun <T> exchange(
        client: OkHttpClient,
        request: Request,
        maxBodyBytes: Long,
        onSuccess: (status: Int, body: ByteArray) -> T,
    ): LinkResult<T> =
        suspendCancellableCoroutine { continuation ->
            val call = client.newCall(request)
            continuation.invokeOnCancellation { call.cancel() }
            call.enqueue(
                object : Callback {
                    override fun onFailure(call: Call, e: IOException) {
                        continuation.resume(LinkResult.Failure(classify(e)))
                    }

                    override fun onResponse(call: Call, response: Response) {
                        val result = try {
                            response.use { interpret(it, maxBodyBytes, onSuccess) }
                        } catch (e: IOException) {
                            LinkResult.Failure(classify(e))
                        }
                        continuation.resume(result)
                    }
                },
            )
        }

    fun <T> interpret(response: Response, maxBodyBytes: Long, onSuccess: (status: Int, body: ByteArray) -> T): LinkResult<T> {
        val status = response.code
        if (response.header(CONTRACT_HEADER) != CONTRACT) {
            return LinkResult.Failure(LinkError.Unexpected(LinkError.Unexpected.Reason.CONTRACT_HEADER, status))
        }
        if (!response.isSuccessful) {
            val code = errorCode(response.body.readBounded(MAX_ERROR_BYTES))
            val api = code?.let { LinkApiError.of(it, status) }
            return LinkResult.Failure(api?.let { LinkError.Api(it) } ?: LinkError.Unexpected(LinkError.Unexpected.Reason.STATUS, status))
        }
        val body = response.body.readBounded(maxBodyBytes)
            ?: return LinkResult.Failure(LinkError.Unexpected(LinkError.Unexpected.Reason.TOO_LARGE, status))
        return try {
            LinkResult.Ok(onSuccess(status, body))
        } catch (_: IllegalArgumentException) {
            // SerializationException is an IllegalArgumentException; its message may echo the body: dropped.
            LinkResult.Failure(LinkError.Unexpected(LinkError.Unexpected.Reason.BODY, status))
        }
    }

    /**
     * No HTTP answer: TLS alerts certificate_revoked / certificate_expired -> Tls.Revoked / Tls.Expired; other handshake failures
     * (pin, SAN, certificate) -> Tls.Other; timeouts -> Timeout; anything else (refused, reset mid-stream) -> Refused.
     */
    fun classify(e: IOException): LinkError.Transport {
        val chain = generateSequence<Throwable>(e) { it.cause }.take(MAX_CAUSES).toList() + e.suppressed
        if (chain.any { it is SSLException }) {
            val text = chain.mapNotNull { it.message }.joinToString(" ").lowercase(Locale.ROOT)
            when {
                ALERT_REVOKED in text -> return LinkError.Transport.Tls.Revoked
                ALERT_EXPIRED in text -> return LinkError.Transport.Tls.Expired
                ALERT in text || chain.any { it is SSLHandshakeException || it is SSLPeerUnverifiedException || it is CertificateException } ->
                    return LinkError.Transport.Tls.Other
            }
        }
        return if (chain.any { it is InterruptedIOException }) LinkError.Transport.Timeout else LinkError.Transport.Refused
    }

    private fun ResponseBody.readBounded(max: Long): ByteArray? {
        if (contentLength() > max) return null
        val source = source()
        if (source.request(max + 1)) return null
        return source.buffer.readByteArray()
    }

    private fun errorCode(body: ByteArray?): String? =
        body?.let {
            runCatching {
                json.parseToJsonElement(it.decodeToString()).jsonObject["error"]?.jsonObject?.get("code")?.jsonPrimitive?.contentOrNull
            }.getOrNull()
        }
}
