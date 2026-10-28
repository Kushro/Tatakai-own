// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.HttpUrl
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody

@Serializable
data class OutputDto(val width: Int, val height: Int)

@Serializable
data class ProfileDto(
    val id: String,
    val label: String,
    val upscale: String,
    val fg: String = "none",
    val output: OutputDto,
    /** `true`, `false` or the string `unknown`. */
    @SerialName("measured_sustain") val measuredSustain: JsonElement = JsonPrimitive("unknown"),
)

@Serializable
data class CapabilitiesDto(
    val contract: String,
    @SerialName("server_version") val serverVersion: String,
    @SerialName("instance_id") val instanceId: String,
    @SerialName("server_name") val serverName: String,
    val profiles: List<ProfileDto>,
    @SerialName("default_profile") val defaultProfile: String,
    @SerialName("client_fg") val clientFg: String = "suppressed",
)

@Serializable
data class SessionCreatedDto(
    @SerialName("session_id") val sessionId: String,
    val generation: Int,
    @SerialName("playback_path") val playbackPath: String,
    @SerialName("duration_secs") val durationSecs: Double? = null,
    @SerialName("segment_secs") val segmentSecs: Int,
    @SerialName("heartbeat_interval_secs") val heartbeatIntervalSecs: Int,
    @SerialName("idle_timeout_secs") val idleTimeoutSecs: Int,
)

@Serializable
data class SessionStateDto(
    val state: String,
    val generation: Int,
    @SerialName("instance_id") val instanceId: String,
    val profile: String,
    val error: String? = null,
)

@Serializable
data class LibraryItemDto(
    val id: String,
    val title: String,
    @SerialName("duration_secs") val durationSecs: Double,
    val width: Int,
    val height: Int,
    @SerialName("size_bytes") val sizeBytes: Long,
)

@Serializable
data class LibraryDto(val items: List<LibraryItemDto>, val truncated: Boolean)

/** `POST /v1/sessions` source. M0 supports `library` and `url` only (openapi.v1.json `SessionCreate`). */
sealed interface SessionSource {
    data class Library(val itemId: String) : SessionSource
    data class Url(val url: String, val headers: Map<String, String>, val extensionId: String? = null, val container: String? = null) : SessionSource
}

/**
 * REST client of Tatakai Link v1 over an already-configured OkHttp client (mTLS + pinned CA in production, plain HTTP against a
 * MockWebServer in tests). Every call returns a [LinkResult]; nothing throws for protocol-level failures and nothing is logged.
 */
class LinkClient(private val baseUrl: HttpUrl, private val client: OkHttpClient) {
    suspend fun capabilities(): LinkResult<CapabilitiesDto> =
        get("v1/capabilities", MAX_JSON) { LinkHttp.json.decodeFromString(CapabilitiesDto.serializer(), it.decodeToString()).also { c -> require(c.contract == "v1") } }

    suspend fun library(): LinkResult<LibraryDto> =
        get("v1/library", MAX_LIBRARY) { LinkHttp.json.decodeFromString(LibraryDto.serializer(), it.decodeToString()) }

    suspend fun createSession(
        source: SessionSource,
        profile: String? = null,
        audioTrack: Int? = null,
        startSecs: Double? = null,
    ): LinkResult<SessionCreatedDto> {
        val body = buildJsonObject {
            put("source", sourceJson(source))
            if (profile != null) put("profile", profile)
            if (audioTrack != null) put("audio_track", audioTrack)
            if (startSecs != null) put("start_secs", startSecs)
        }.toString().toByteArray(Charsets.UTF_8)
        val request = LinkHttp.request(url("v1/sessions")).post(body.toRequestBody(LinkHttp.JSON_TYPE)).build()
        return LinkHttp.exchange(client, request, MAX_JSON) { _, b ->
            LinkHttp.json.decodeFromString(SessionCreatedDto.serializer(), b.decodeToString()).also {
                require(SESSION_ID.matches(it.sessionId) && it.playbackPath.startsWith("/v1/sessions/") && ".." !in it.playbackPath)
            }
        }
    }

    suspend fun sessionState(sessionId: String): LinkResult<SessionStateDto> {
        if (!SESSION_ID.matches(sessionId)) return LinkResult.Failure(LinkError.Api(LinkApiError.RequestInvalid))
        return get("v1/sessions/$sessionId", MAX_JSON) { LinkHttp.json.decodeFromString(SessionStateDto.serializer(), it.decodeToString()) }
    }

    suspend fun deleteSession(sessionId: String): LinkResult<Unit> {
        if (!SESSION_ID.matches(sessionId)) return LinkResult.Failure(LinkError.Api(LinkApiError.RequestInvalid))
        return LinkHttp.exchange(client, LinkHttp.request(url("v1/sessions/$sessionId")).delete().build(), MAX_JSON) { _, _ -> Unit }
    }

    /** `POST /v1/sessions/{id}/position`: the 4 s heartbeat (120 s without one closes the session). */
    suspend fun heartbeat(sessionId: String, positionSecs: Double, paused: Boolean): LinkResult<Unit> {
        if (!SESSION_ID.matches(sessionId)) return LinkResult.Failure(LinkError.Api(LinkApiError.RequestInvalid))
        val body = buildJsonObject {
            put("position_secs", positionSecs.coerceAtLeast(0.0))
            put("paused", paused)
        }.toString().toByteArray(Charsets.UTF_8)
        val request = LinkHttp.request(url("v1/sessions/$sessionId/position")).post(body.toRequestBody(LinkHttp.JSON_TYPE)).build()
        return LinkHttp.exchange(client, request, MAX_JSON) { _, _ -> Unit }
    }

    /** `DELETE /v1/devices/self`: this phone revokes its own certificate. */
    suspend fun forgetSelf(): LinkResult<Unit> =
        LinkHttp.exchange(client, LinkHttp.request(url("v1/devices/self")).delete().build(), MAX_JSON) { _, _ -> Unit }

    /** Absolute URL of a `playback_path` returned by the PC (same origin as this client). */
    fun playbackUrl(playbackPath: String): String = baseUrl.newBuilder().encodedPath(playbackPath).build().toString()

    private suspend fun <T> get(path: String, max: Long, decode: (ByteArray) -> T): LinkResult<T> =
        LinkHttp.exchange(client, LinkHttp.request(url(path)).get().build(), max) { _, b -> decode(b) }

    private fun url(path: String): HttpUrl = baseUrl.newBuilder().addPathSegments(path).build()

    private fun sourceJson(source: SessionSource): JsonObject = when (source) {
        is SessionSource.Library -> buildJsonObject { put("kind", "library"); put("item_id", source.itemId) }
        is SessionSource.Url -> buildJsonObject {
            put("kind", "url")
            put("url", source.url)
            if (source.headers.isNotEmpty()) put("headers", JsonObject(source.headers.mapValues { JsonPrimitive(it.value) }))
            put("extension_id", source.extensionId?.let { JsonPrimitive(it) } ?: JsonNull)
            put("container", source.container?.let { JsonPrimitive(it) } ?: JsonNull)
        }.let { obj -> JsonObject(obj.filterValues { it !is JsonNull }) }
    }

    companion object {
        private const val MAX_JSON = 256L * 1024
        private const val MAX_LIBRARY = 8L * 1024 * 1024
        val SESSION_ID = Regex("""[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}""")
    }
}
