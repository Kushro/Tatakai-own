// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import okhttp3.HttpUrl
import okhttp3.OkHttpClient
import okhttp3.RequestBody.Companion.toRequestBody
import java.util.concurrent.TimeUnit

// Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.11, worker r): the phone side of the CANONICAL remote-control
// wire of desktop `apps/desktop-native/crates/tk-link/src/control.rs` (worker k). Neither side changes it alone.

/** `GET /v1/player` body (camelCase on the wire). */
@Serializable
data class PlayerStateDto(
    val playing: Boolean,
    val title: String? = null,
    val episode: Int? = null,
    val positionSecs: Int = 0,
    val durationSecs: Int? = null,
    val mediaId: String? = null,
)

/**
 * One `GET /v1/events` event (`type`-tagged: `handoff`, and since 2026-11 `history_resync` with [mode]). Unknown types and keys are
 * skipped (`LinkHttp.json` ignores unknown keys), so a newer PC never breaks this phone.
 */
@Serializable
data class PhoneEventDto(
    val type: String,
    val sessionId: String? = null,
    val mediaId: String? = null,
    val episode: Int? = null,
    val positionSecs: Int = 0,
    val title: String? = null,
    /** `history_resync`: `push_all` / `pull_replace`. */
    val mode: String? = null,
)

@Serializable
data class EventsPageDto(val events: List<PhoneEventDto> = emptyList())

/** `POST /v1/control` bodies (`ControlCommand` of control.rs). Pure: JVM-tested. */
internal object ControlWire {
    const val PREV = "prev"
    const val TOGGLE = "toggle"
    const val NEXT = "next"
    const val OPEN = "open"
    const val EVENT_HANDOFF = "handoff"
    const val EVENT_HISTORY_RESYNC = "history_resync"

    /** `EVENTS_MAX_WAIT_SECS` of control.rs. */
    const val MAX_WAIT_SECS = 25

    fun simple(command: String): String = buildJsonObject { put("command", command) }.toString()

    fun open(mediaId: String, episode: Int, positionSecs: Int): String = buildJsonObject {
        put("command", OPEN)
        put("mediaId", mediaId)
        // control.rs `valid`: episodes start at 1.
        put("episode", episode.coerceAtLeast(1))
        put("positionSecs", positionSecs.coerceAtLeast(0))
    }.toString()
}

/** Extra routes of [LinkClient] for wave B, kept apart so the wave A file stays readable. */
internal class LinkControlClient(private val baseUrl: HttpUrl, private val client: OkHttpClient) {
    /** `GET /v1/player`; `404` (`no_player`, or a PC without remote control) = null. */
    suspend fun playerState(): LinkResult<PlayerStateDto?> {
        val r = LinkHttp.exchange(client, LinkHttp.request(url("v1/player")).get().build(), MAX_JSON) { _, b ->
            LinkHttp.json.decodeFromString(PlayerStateDto.serializer(), b.decodeToString())
        }
        return when {
            r is LinkResult.Failure && r.error.status() == 404 -> LinkResult.Ok(null)
            r is LinkResult.Ok -> LinkResult.Ok(r.value)
            else -> r as LinkResult.Failure
        }
    }

    /** `POST /v1/control` (204). `409 no_player` comes back as a failure with status 409. */
    suspend fun control(body: String): LinkResult<Unit> {
        val request = LinkHttp.request(url("v1/control")).post(body.toByteArray(Charsets.UTF_8).toRequestBody(LinkHttp.JSON_TYPE)).build()
        return LinkHttp.exchange(client, request, MAX_JSON) { _, _ -> Unit }
    }

    /**
     * `GET /v1/events?wait=` long-poll: `204` = nothing (empty list), `200` = the queued events. Parsed event by event, so one event
     * this version cannot read (a future shape) never drops the others.
     */
    suspend fun events(wait: Int = ControlWire.MAX_WAIT_SECS): LinkResult<List<PhoneEventDto>> {
        val w = wait.coerceIn(1, ControlWire.MAX_WAIT_SECS)
        val longPoll = client.newBuilder()
            .readTimeout((w + LONG_POLL_SLACK_SECS).toLong(), TimeUnit.SECONDS)
            .callTimeout((w + LONG_POLL_SLACK_SECS * 2).toLong(), TimeUnit.SECONDS)
            .build()
        val target = url("v1/events").newBuilder().addQueryParameter("wait", w.toString()).build()
        return LinkHttp.exchange(longPoll, LinkHttp.request(target).get().build(), MAX_JSON) { status, b ->
            if (status == 204 || b.isEmpty()) emptyList() else LinkWireMapping.events(b.decodeToString())
        }
    }

    private fun url(path: String): HttpUrl = baseUrl.newBuilder().addPathSegments(path).build()

    companion object {
        private const val MAX_JSON = 64L * 1024
        private const val LONG_POLL_SLACK_SECS = 10
    }
}

/** HTTP status of a failure when the PC answered (null for transport errors / not paired). */
internal fun LinkError.status(): Int? = when (this) {
    is LinkError.Api -> error.httpStatus
    is LinkError.Unexpected -> httpStatus
    else -> null
}

/** DTO -> model mapping of the wave B routes (wire keys positionSecs / durationSecs / mediaId). Pure, JVM-tested. */
internal object LinkWireMapping {
    /** `EventsPage` JSON -> the events this version reads; an element that does not decode is skipped. */
    fun events(body: String): List<PhoneEventDto> {
        val root = LinkHttp.json.parseToJsonElement(body) as? kotlinx.serialization.json.JsonObject ?: return emptyList()
        val list = root["events"] as? kotlinx.serialization.json.JsonArray ?: return emptyList()
        return list.mapNotNull { e -> runCatching { LinkHttp.json.decodeFromJsonElement(PhoneEventDto.serializer(), e) }.getOrNull() }
    }

    fun playerState(dto: PlayerStateDto): app.tatakai.next.model.RemotePlayerState = app.tatakai.next.model.RemotePlayerState(
        playing = dto.playing,
        title = dto.title?.takeIf { it.isNotBlank() },
        episode = dto.episode,
        positionSec = dto.positionSecs.coerceAtLeast(0),
        durationSec = dto.durationSecs?.takeIf { it > 0 },
        animeId = dto.mediaId?.takeIf { it.isNotBlank() },
    )

    /** A `history_resync` event with a known mode; anything else is dropped. */
    fun historyResync(dto: PhoneEventDto): HistoryResyncMode? =
        if (dto.type == ControlWire.EVENT_HISTORY_RESYNC) HistoryResyncMode.fromWire(dto.mode) else null

    /** A `handoff` event with a well-formed session id; anything else (unknown type, bad id) is dropped. */
    fun handoff(dto: PhoneEventDto, nowMs: Long = System.currentTimeMillis()): app.tatakai.next.model.LinkHandoff? {
        if (dto.type != ControlWire.EVENT_HANDOFF) return null
        val id = dto.sessionId?.takeIf { LinkClient.SESSION_ID.matches(it) } ?: return null
        return app.tatakai.next.model.LinkHandoff(
            sessionId = id,
            animeId = dto.mediaId?.takeIf { it.isNotBlank() },
            episode = dto.episode,
            positionSec = dto.positionSecs.coerceAtLeast(0),
            title = dto.title?.takeIf { it.isNotBlank() },
            receivedAtMs = nowMs,
        )
    }
}
