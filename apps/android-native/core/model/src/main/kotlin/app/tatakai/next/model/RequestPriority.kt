// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.withContext
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import kotlin.coroutines.AbstractCoroutineContextElement
import kotlin.coroutines.CoroutineContext
import kotlin.coroutines.coroutineContext

/**
 * 0.3.1 (AniList gateway): how urgent an AniList request is. The shared gateway (`:core:network` `AniListGateway`) lets
 * [INTERACTIVE] requests (details, search, player, list writes) go before [BACKGROUND] ones (Home rows, recommendations, calendar,
 * social polling, WorkManager jobs) and keeps a few slots of the per-minute budget for the interactive ones.
 *
 * Call sites pick the priority with [inBackground]; the request builders attach [currentRequestPriority] as an OkHttp tag
 * (`Request.Builder.tag(RequestPriority::class.java, …)`). Untagged requests are interactive.
 */
enum class RequestPriority { INTERACTIVE, BACKGROUND }

/** Coroutine context element carrying the [RequestPriority] down to the HTTP request builders. */
class RequestPriorityElement(val priority: RequestPriority) : AbstractCoroutineContextElement(Key) {
    companion object Key : CoroutineContext.Key<RequestPriorityElement>
}

/** Priority of the AniList requests made by the calling coroutine ([RequestPriority.INTERACTIVE] unless inside [inBackground]). */
suspend fun currentRequestPriority(): RequestPriority = coroutineContext[RequestPriorityElement]?.priority ?: RequestPriority.INTERACTIVE

/** Runs [block] with its AniList requests marked [RequestPriority.BACKGROUND] (no dispatcher change). */
suspend fun <T> inBackground(block: suspend () -> T): T = withContext(RequestPriorityElement(RequestPriority.BACKGROUND)) { block() }

/** Parsing of the rate-limit headers AniList sends (`X-RateLimit-Limit` / `-Remaining` / `-Reset`, `Retry-After`). Pure. */
object RateLimitHeaders {
    const val LIMIT = "X-RateLimit-Limit"
    const val REMAINING = "X-RateLimit-Remaining"
    const val RESET = "X-RateLimit-Reset"
    const val RETRY_AFTER = "Retry-After"

    /**
     * Set by the AniList gateway on the 429s it hands back: it already waited / retried what was short, so callers must not retry
     * again (they surface `RateLimited` with the wait).
     */
    const val GATEWAY = "X-Tatakai-AniList-Gateway"

    /** A non-negative integer header (`X-RateLimit-Limit` / `-Remaining`), null when absent or malformed. */
    fun count(value: String?): Int? = value?.trim()?.toIntOrNull()?.takeIf { it >= 0 }

    /** `X-RateLimit-Reset` (unix seconds) as epoch millis, null when absent or malformed. */
    fun resetAtMs(value: String?): Long? = value?.trim()?.toLongOrNull()?.takeIf { it > 0 }?.let { it * 1000 }

    /**
     * `Retry-After` as a wait in millis from [nowMs]: delta-seconds (`"30"`) or an HTTP date (RFC 1123); null when absent or
     * malformed. A date in the past gives 0.
     */
    fun retryAfterMs(value: String?, nowMs: Long): Long? {
        val v = value?.trim()?.takeIf { it.isNotEmpty() } ?: return null
        v.toLongOrNull()?.let { return if (it < 0) null else it * 1000 }
        return runCatching { ZonedDateTime.parse(v, DateTimeFormatter.RFC_1123_DATE_TIME).toInstant().toEpochMilli() }
            .getOrNull()?.let { (it - nowMs).coerceAtLeast(0) }
    }
}
