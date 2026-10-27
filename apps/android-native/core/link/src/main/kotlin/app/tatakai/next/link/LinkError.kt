// SPDX-License-Identifier: AGPL-3.0-or-later
// Structure adapted from Enigmata (app.enigmata.temple.client.TempleError), same author, AGPL-3.0-or-later. See NOTICES-B4.md.
package app.tatakai.next.link

import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException

/** Outcome of a client call: a value, or a typed [LinkError]. */
sealed interface LinkResult<out T> {
    data class Ok<out T>(val value: T) : LinkResult<T>
    data class Failure(val error: LinkError) : LinkResult<Nothing>

    fun getOrNull(): T? = (this as? Ok)?.value
    fun errorOrNull(): LinkError? = (this as? Failure)?.error
}

inline fun <T, R> LinkResult<T>.map(transform: (T) -> R): LinkResult<R> =
    when (this) {
        is LinkResult.Ok -> LinkResult.Ok(transform(value))
        is LinkResult.Failure -> this
    }

/**
 * The 26 error codes of contracts/link/openapi.v1.json (`x-errors`) with their HTTP status. A response whose code and status do not
 * match one row is [LinkError.Unexpected], never one of these. [m0] = produced by the M0 server.
 */
enum class LinkApiError(val code: String, val httpStatus: Int, val m0: Boolean = true) {
    ClientCertificateRequired("client_certificate_required", 401),
    DeviceNotAuthorized("device_not_authorized", 403),
    PairingClosed("pairing_closed", 409),
    PairingCodeInvalid("pairing_code_invalid", 403),
    PairingAttemptsExceeded("pairing_attempts_exceeded", 429),
    CsrInvalid("csr_invalid", 400),
    RequestInvalid("request_invalid", 400),
    BodyTooLarge("body_too_large", 413),
    BodyTimeout("body_timeout", 408),
    ProfileUnavailable("profile_unavailable", 422),
    PolicyUnavailable("policy_unavailable", 422, m0 = false),
    LibraryItemNotFound("library_item_not_found", 404),
    SourceUnavailable("source_unavailable", 503),
    SessionNotFound("session_not_found", 404),
    SessionGone("session_gone", 410),
    SessionNotReady("session_not_ready", 409),
    SegmentWaitTimeout("segment_wait_timeout", 503),
    RangeNotFound("range_not_found", 404, m0 = false),
    RangeInvalid("range_invalid", 400, m0 = false),
    SettingsInvalid("settings_invalid", 400, m0 = false),
    SettingsConflict("settings_conflict", 409, m0 = false),
    ServerBusy("server_busy", 503),
    Internal("internal", 500),
    UrlSourceDenied("url_source_denied", 403),
    ExtensionUnavailable("extension_unavailable", 422, m0 = false),
    HandoffRejected("handoff_rejected", 409, m0 = false),
    ;

    companion object {
        private val byCode = entries.associateBy { it.code }

        /** The contract error for [code] answered with [httpStatus]; null if they are not a row of the table. */
        fun of(code: String, httpStatus: Int): LinkApiError? = byCode[code]?.takeIf { it.httpStatus == httpStatus }
    }
}

/** Why a call failed. The server's message never travels (only the code is mapped); [toString] is safe to log. */
sealed interface LinkError {
    data class Api(val error: LinkApiError) : LinkError

    /** No HTTP answer. */
    sealed interface Transport : LinkError {
        /** Refused / unreachable / cut before an answer: the PC is offline. */
        data object Refused : Transport
        data object Timeout : Transport

        sealed interface Tls : Transport {
            /** TLS alert `certificate_revoked`: this phone was revoked on the PC. */
            data object Revoked : Tls

            /** TLS alert `certificate_expired`: the pairing expired; pair again. */
            data object Expired : Tls

            /** Any other handshake failure (pin, SAN, protocol). */
            data object Other : Tls
        }
    }

    /** An answer outside contract v1. */
    data class Unexpected(val reason: Reason, val httpStatus: Int? = null) : LinkError {
        enum class Reason { CONTRACT_HEADER, STATUS, BODY, TOO_LARGE }
    }

    /** No pairing on this phone; nothing was sent. */
    data object NotPaired : LinkError
}

class LinkException(val error: LinkError) : Exception(error.toString())

/** Maps a failure to the app-wide [AppException] (the original [LinkError] stays as `cause` for UI that needs the exact code). */
fun LinkError.toAppException(): AppException {
    val app = when (this) {
        is LinkError.Transport.Refused, is LinkError.Transport.Timeout -> AppError.Offline
        is LinkError.Api -> AppError.Http(error.httpStatus)
        is LinkError.Unexpected -> AppError.Http(httpStatus ?: 0)
        is LinkError.Transport.Tls.Revoked -> AppError.Unknown("link:revoked")
        is LinkError.Transport.Tls.Expired -> AppError.Unknown("link:expired")
        is LinkError.Transport.Tls.Other -> AppError.Unknown("link:tls")
        is LinkError.NotPaired -> AppError.Unknown("link:not_paired")
    }
    return AppException(app, LinkException(this))
}
