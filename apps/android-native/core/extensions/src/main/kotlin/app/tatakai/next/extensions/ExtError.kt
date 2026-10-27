// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

/**
 * Failure of a host call. [status] mirrors the legacy host statuses (routes.v1.json `errorStatuses`):
 * 400 missing param, 404 unknown namespace/route/provider, 501 not implemented by the bundle, 502 bundle threw.
 */
sealed class ExtError(message: String, val status: Int, cause: Throwable? = null) : Exception(message, cause) {
    class BadRequest(message: String) : ExtError(message, 400)
    class NotFound(message: String) : ExtError(message, 404)
    class NotImplemented(message: String) : ExtError(message, 501)
    class BundleError(message: String, cause: Throwable? = null) : ExtError(message, 502, cause)

    /** Exceeded the 15 s wall-clock budget of one call; the runtime is discarded. */
    class Timeout : ExtError("Extension call timed out", 504)

    /** Exceeded the 64 MiB memory limit; the runtime is discarded. */
    class OutOfMemory : ExtError("Extension exceeded its memory limit", 507)

    /** `compat = unsupported`, or the extension is disabled. */
    class Unsupported(message: String) : ExtError(message, 501)

    /** `fetch` to a host outside the manifest permissions (or blocked by SSRF policy). */
    class PermissionDenied(message: String = MESSAGE) : ExtError(message, 403) {
        companion object {
            const val MESSAGE = "Network permission denied"
        }
    }
}
