// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.put
import okhttp3.Call
import okhttp3.Callback
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.ResponseBody
import java.io.IOException
import kotlin.coroutines.resume
import kotlin.coroutines.resumeWithException

/**
 * Client of the integrated extension server `/api/server/v1` (docs/redesign/13 §3.2, §3.10). Envelope `{ success, data }` /
 * `{ success: false, error: { code, message } }`; authentication `Authorization: Bearer <token>`.
 *
 * - [HostGuard] runs before every request (and [okHttp] must not follow redirects: see [ExtensionServerModule]).
 * - Nothing is logged; tokens never leave the `Authorization` header; the server's error messages are dropped, only the code is kept.
 * - [resolve] streams `application/x-ndjson`: every line is a complete snapshot (D6a), so the latest one is always the full state.
 */
class ExtensionServerClient(
    private val okHttp: OkHttpClient,
    private val json: Json = DefaultJson,
    private val io: CoroutineDispatcher = Dispatchers.IO,
) {
    suspend fun health(url: String): ServerHealth = call(url, "/health", token = null, body = null, ServerHealth.serializer())

    /** `POST /auth/pair` with the one-time code of a [PairingLink]. */
    suspend fun pair(url: String, code: String, label: String): ServerToken =
        call(url, "/auth/pair", null, buildJsonObject { put("code", code); put("label", label) }, ServerToken.serializer())

    /** `POST /auth/login` as viewer. */
    suspend fun login(url: String, password: String, label: String): ServerToken =
        call(
            url, "/auth/login", null,
            buildJsonObject { put("role", "viewer"); put("password", password); put("label", label) },
            ServerToken.serializer(),
        )

    suspend fun logout(conn: ExtensionServerConnection) {
        call(conn.url, "/auth/logout", conn.token, JsonObject(emptyMap()), JsonElement.serializer())
    }

    suspend fun sources(conn: ExtensionServerConnection): List<ServerSource> =
        call(conn.url, "/sources", conn.token, null, ListSerializer(ServerSource.serializer()))

    suspend fun invoke(conn: ExtensionServerConnection, sourceId: String, op: String, args: JsonObject): JsonElement =
        call(
            conn.url, "/invoke", conn.token,
            buildJsonObject {
                put("sourceId", sourceId)
                put("op", op)
                put("args", args)
                put("progressive", op == "chapters" || op == "novelChapters")
            },
            JsonElement.serializer(),
        )

    /**
     * `POST /resolve` with `accept: application/x-ndjson`. Emits each snapshot as it arrives and completes when the server closes the
     * stream. Cancelling the collector cancels the HTTP call. A non-2xx answer fails the flow with [ExtensionServerException]; a body
     * above [MAX_STREAM_BYTES] fails with `response_too_large`. A server answering plain JSON (no streaming) is accepted as one snapshot.
     */
    fun resolve(conn: ExtensionServerConnection, request: JsonObject): Flow<ResolveSnapshot> = callbackFlow {
        val built = try {
            build(conn.url, "/resolve", conn.token, request)
        } catch (e: IllegalArgumentException) {
            throw ExtensionServerException(ExtensionServerException.INVALID_URL, cause = e)
        }
        val call = okHttp.newCall(built.newBuilder().header("Accept", NDJSON).build())
        launch(io) {
            try {
                call.execute().use { response ->
                    if (!response.isSuccessful) throw errorOf(response)
                    val type = response.header("Content-Type").orEmpty()
                    if (!type.contains("ndjson")) {
                        val element = unwrap(readBounded(response.body, MAX_JSON_BYTES), response.code)
                        send(decode(ResolveSnapshot.serializer(), element, response.code))
                    } else {
                        val source = response.body.source()
                        var total = 0L
                        while (true) {
                            val line = source.readUtf8Line() ?: break
                            total += line.length + 1
                            if (total > MAX_STREAM_BYTES) throw ExtensionServerException(ExtensionServerException.TOO_LARGE, response.code)
                            if (line.isBlank()) continue
                            send(parseSnapshotLine(line))
                        }
                    }
                }
                close()
            } catch (e: ExtensionServerException) {
                close(e)
            } catch (e: IOException) {
                close(ExtensionServerException(ExtensionServerException.UNREACHABLE, cause = e))
            } catch (e: IllegalArgumentException) {
                close(ExtensionServerException(ExtensionServerException.BAD_RESPONSE, cause = e))
            }
        }
        awaitClose { call.cancel() }
    }

    /** One NDJSON line -> snapshot; a malformed line is a protocol error. */
    fun parseSnapshotLine(line: String): ResolveSnapshot = try {
        json.decodeFromString(ResolveSnapshot.serializer(), line)
    } catch (e: IllegalArgumentException) {
        throw ExtensionServerException(ExtensionServerException.BAD_RESPONSE, cause = e)
    }

    // ---- plumbing ---------------------------------------------------------------------------------------------------------------

    private fun build(base: String, path: String, token: String?, body: JsonObject?): Request {
        val url = base.trimEnd('/') + API + path
        HostGuard.require(url)
        return Request.Builder()
            .url(url)
            .header("Accept", "application/json")
            .apply { if (token != null) header("Authorization", "Bearer $token") }
            .apply {
                if (body == null) get() else post(json.encodeToString(JsonObject.serializer(), body).toRequestBody(JSON_TYPE))
            }
            .build()
    }

    private suspend fun <T> call(base: String, path: String, token: String?, body: JsonObject?, serializer: KSerializer<T>): T {
        val request = try {
            build(base, path, token, body)
        } catch (e: IllegalArgumentException) {
            throw ExtensionServerException(ExtensionServerException.INVALID_URL, cause = e)
        }
        val (status, bytes) = execute(request)
        return decode(serializer, unwrap(bytes, status), status)
    }

    private fun <T> decode(serializer: KSerializer<T>, element: JsonElement, status: Int): T = try {
        json.decodeFromJsonElement(serializer, element)
    } catch (e: IllegalArgumentException) {
        throw ExtensionServerException(ExtensionServerException.BAD_RESPONSE, status, e)
    }

    /** `{ success: true, data }` -> data. Bodies without the envelope are taken as the data itself (tolerant, like the TS client). */
    private fun unwrap(bytes: ByteArray, status: Int): JsonElement {
        val root = try {
            json.parseToJsonElement(bytes.decodeToString())
        } catch (e: IllegalArgumentException) {
            throw ExtensionServerException(ExtensionServerException.BAD_RESPONSE, status, e)
        }
        val obj = root as? JsonObject ?: return root
        val success = (obj["success"] as? JsonPrimitive)?.contentOrNull
        return when (success) {
            "true" -> obj["data"] ?: JsonObject(emptyMap())
            "false" -> throw ExtensionServerException(errorCode(obj) ?: ExtensionServerException.BAD_RESPONSE, status)
            else -> root
        }
    }

    /** Runs [request] off the caller's thread; cancelling the coroutine cancels the call. Non-2xx -> [ExtensionServerException]. */
    private suspend fun execute(request: Request): Pair<Int, ByteArray> = suspendCancellableCoroutine { continuation ->
        val call = okHttp.newCall(request)
        continuation.invokeOnCancellation { call.cancel() }
        call.enqueue(
            object : Callback {
                override fun onFailure(call: Call, e: IOException) {
                    continuation.resumeWithException(ExtensionServerException(ExtensionServerException.UNREACHABLE, cause = e))
                }

                override fun onResponse(call: Call, response: Response) {
                    try {
                        response.use {
                            if (!it.isSuccessful) throw errorOf(it)
                            continuation.resume(it.code to readBounded(it.body, MAX_JSON_BYTES))
                        }
                    } catch (e: ExtensionServerException) {
                        continuation.resumeWithException(e)
                    } catch (e: IOException) {
                        continuation.resumeWithException(ExtensionServerException(ExtensionServerException.UNREACHABLE, cause = e))
                    }
                }
            },
        )
    }

    private fun errorOf(response: Response): ExtensionServerException {
        val code = runCatching {
            val bytes = readBounded(response.body, MAX_ERROR_BYTES)
            (json.parseToJsonElement(bytes.decodeToString()) as? JsonObject)?.let(::errorCode)
        }.getOrNull()
        return ExtensionServerException(code ?: "http_${response.code}", response.code)
    }

    private fun errorCode(obj: JsonObject): String? =
        ((obj["error"] as? JsonObject)?.get("code") as? JsonPrimitive)?.contentOrNull?.takeIf { CODE.matches(it) }

    private fun readBounded(body: ResponseBody, max: Long): ByteArray {
        if (body.contentLength() > max) throw ExtensionServerException(ExtensionServerException.TOO_LARGE)
        val source = body.source()
        if (source.request(max + 1)) throw ExtensionServerException(ExtensionServerException.TOO_LARGE)
        return source.buffer.readByteArray()
    }

    companion object {
        const val API = "/api/server/v1"
        const val NDJSON = "application/x-ndjson"
        const val MAX_JSON_BYTES = 8L * 1024 * 1024
        const val MAX_STREAM_BYTES = 8L * 1024 * 1024
        private const val MAX_ERROR_BYTES = 16L * 1024
        private val JSON_TYPE = "application/json".toMediaType()
        private val CODE = Regex("[a-z0-9_]{1,64}")

        val DefaultJson = Json {
            ignoreUnknownKeys = true
            explicitNulls = false
            coerceInputValues = true
            isLenient = false
        }
    }
}
