// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

// Wire types of /api/server/v1 (server/src/contracts.ts, docs/redesign/13 §3.2). Every field the app does not need is ignored.

/** One linked extension server, as stored on this phone (same shape as the classic client's `ServerConnection`). */
@Serializable
data class ExtensionServerConnection(
    val id: String,
    val serverId: String,
    /** Origin `scheme://host[:port]` that passed [HostGuard]. */
    val url: String,
    val name: String,
    val token: String,
    val enabled: Boolean = true,
) {
    /** Never prints the token. */
    override fun toString(): String = "ExtensionServerConnection(id=$id, serverId=$serverId, url=$url, name=$name, enabled=$enabled)"
}

@Serializable
data class ServerHealth(
    val protocolVersion: Int = 0,
    val serverId: String? = null,
    val name: String = "",
    val platform: String? = null,
    val setupRequired: Boolean = false,
    val accessPasswordRequired: Boolean = false,
    /** D4b: desktop server without viewer password, only paired devices are admitted. */
    val pairingRequired: Boolean = false,
    val dashboardPath: String? = null,
    val desktop: Boolean = false,
)

@Serializable
data class ServerToken(val token: String, val role: String = "viewer", val deviceId: String = "") {
    override fun toString(): String = "ServerToken(role=$role, deviceId=$deviceId)"
}

@Serializable
data class ServerSource(
    val id: String,
    val extensionId: String = "",
    val name: String = "",
    val ecosystem: String = "",
    /** `watch` | `read` | `novel`. */
    val kind: String = "watch",
    val enabled: Boolean = true,
    val available: Boolean = true,
    val reason: String = "",
    val languages: List<String> = emptyList(),
)

/**
 * parity wave B (16 §3.4, worker t; checklist server.api.extensions): one extension installed on the PC (`GET /extensions`). The native
 * server (desktop tk-server) answers `{ id, name, version, type, sources: [...] }`; the Node server `{ id, manifestId, ecosystem, name,
 * version, enabled, kind }`. Every field is optional on the wire.
 */
@Serializable
data class ServerExtension(
    val id: String,
    val name: String = "",
    val version: String = "",
    val type: String = "",
    val ecosystem: String = "",
    val kind: String = "",
    val enabled: Boolean = true,
    /** Source ids (or descriptors reduced to their id) the extension exposes. */
    val sources: List<JsonElement> = emptyList(),
)

/** Per-provider progress line of `/resolve`. */
@Serializable
data class ResolveStatus(
    val provider: String = "",
    val name: String = "",
    val ecosystem: String = "",
    /** `loading` | `ok` | `empty` | `partial` | `error`. */
    val status: String = "",
    val count: Int = 0,
    val durationMs: Long = 0,
    val error: String? = null,
)

/**
 * One NDJSON line of `/resolve` (D6a: each line is a complete snapshot, not a delta) or the aggregated JSON answer. [sources] are kept
 * raw: [ServerCandidates] maps them with the same rules as the installed extensions.
 */
@Serializable
data class ResolveSnapshot(
    val sources: List<JsonElement> = emptyList(),
    val subtitles: List<JsonElement> = emptyList(),
    val statuses: List<ResolveStatus> = emptyList(),
) {
    /** The source entries that are JSON objects (anything else is ignored). */
    val sourceObjects: List<JsonObject> get() = sources.filterIsInstance<JsonObject>()

    /** Whether every provider finished (none still `loading`). */
    val complete: Boolean get() = statuses.none { it.status == "loading" }
}

/** Reachability of a linked server as last observed by this app (memory only). */
sealed interface ServerStatus {
    data object Unknown : ServerStatus
    data object Checking : ServerStatus
    data class Online(val name: String, val sourceCount: Int?) : ServerStatus
    data object Offline : ServerStatus

    /** The PC revoked this phone's token (or it expired): reconnect by pairing again. */
    data object Unauthorized : ServerStatus
}

/**
 * Failure of an extension server call. [code] is the server's `error.code` or one of the client codes below; the server's free-text
 * message is never kept (it could echo URLs). [status] is the HTTP status when there was an answer.
 */
class ExtensionServerException(val code: String, val status: Int? = null, cause: Throwable? = null) :
    Exception("extension server: $code", cause) {
    companion object {
        const val INSECURE_HOST = "insecure_host"
        const val INVALID_LINK = "invalid_link"
        const val INVALID_URL = "invalid_url"
        const val UNREACHABLE = "unreachable"
        const val INCOMPATIBLE = "incompatible"
        const val BAD_RESPONSE = "bad_response"
        const val TOO_LARGE = "response_too_large"

        // Server codes the UI distinguishes (server/src/core.mjs).
        const val SETUP_REQUIRED = "setup_required"
        const val INVALID_PASSWORD = "invalid_password"
        const val PAIRING_EXPIRED = "pairing_expired"
        const val PAIRING_REQUIRED = "pairing_required"
        const val LOGIN_THROTTLED = "login_throttled"
        const val AUTHENTICATION_REQUIRED = "authentication_required"
    }

    /** 401 on an authenticated route: the device token is no longer valid. */
    val unauthorized: Boolean get() = status == 401 && code != INVALID_PASSWORD && code != PAIRING_EXPIRED && code != PAIRING_REQUIRED
}
