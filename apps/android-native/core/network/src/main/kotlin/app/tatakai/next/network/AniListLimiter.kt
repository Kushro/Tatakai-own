// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.network

import app.tatakai.next.model.RequestPriority

/**
 * 0.3.1: the budget math of the shared AniList gateway, pure (the caller passes the clock and holds the lock).
 *
 * AniList documents 90 requests/min per IP but has run for a long time in a "degraded" mode of ~30/min, so the budget starts at
 * [defaultLimit] and follows `X-RateLimit-Limit` / `X-RateLimit-Remaining` once a response carries them. Three rules, in order:
 * 1. after a 429 nothing is sent until the cool-down (`Retry-After`, else `X-RateLimit-Reset`, else [default429Ms]) ends;
 * 2. when the last known `Remaining` is at or below the reserve, wait until `X-RateLimit-Reset` (else until the oldest send of the
 *    local window leaves it);
 * 3. never more than `limit` sends in a sliding [windowMs] window.
 * [RequestPriority.BACKGROUND] requests keep [backgroundReserve] slots free for the interactive ones in rules 2 and 3.
 */
class AniListLimiter(
    private val defaultLimit: Int = DEFAULT_LIMIT,
    private val windowMs: Long = WINDOW_MS,
    private val backgroundReserve: Int = BACKGROUND_RESERVE,
    private val default429Ms: Long = DEFAULT_429_MS,
) {
    var limit: Int = defaultLimit
        private set
    var remaining: Int? = null
        private set
    private var remainingAtMs: Long = 0
    var resetAtMs: Long? = null
        private set
    var cooldownUntilMs: Long = 0
        private set
    var last429AtMs: Long? = null
        private set
    var total: Long = 0
        private set

    private val sent = ArrayDeque<Long>()

    private fun prune(now: Long) {
        while (sent.isNotEmpty() && sent.first() <= now - windowMs) sent.removeFirst()
    }

    /** Sends inside the current window. */
    fun sentInWindow(now: Long): Int {
        prune(now)
        return sent.size
    }

    /** How long a request of [priority] must wait before it may be sent; 0 = now. Does not record anything. */
    fun waitMs(now: Long, priority: RequestPriority): Long {
        prune(now)
        if (cooldownUntilMs > now) return cooldownUntilMs - now
        val reserve = if (priority == RequestPriority.BACKGROUND) backgroundReserve else 0
        // a Remaining older than the window, or from before a reset that has passed, says nothing any more
        val resetPassed = resetAtMs?.let { it > remainingAtMs && it <= now } == true
        val rem = remaining?.takeIf { now - remainingAtMs < windowMs && !resetPassed }
        if (rem != null && rem <= reserve) {
            resetAtMs?.takeIf { it > now }?.let { return it - now }
            sent.firstOrNull()?.let { return (it + windowMs - now).coerceAtLeast(1) }
        }
        val cap = (limit - reserve).coerceAtLeast(1)
        if (sent.size >= cap) return (sent[sent.size - cap] + windowMs - now).coerceAtLeast(1)
        return 0
    }

    /** [waitMs] and, when it is 0, records the send (atomic for the caller's lock). */
    fun tryAcquire(now: Long, priority: RequestPriority): Long {
        val w = waitMs(now, priority)
        if (w == 0L) {
            sent.addLast(now)
            total++
            remaining = remaining?.let { (it - 1).coerceAtLeast(0) }
        }
        return w
    }

    /**
     * Learns from a response. Returns the cool-down a 429 imposes (millis from [now]); 0 for any other status. [retryAfterMs] /
     * [resetAtMs] come from [app.tatakai.next.model.RateLimitHeaders].
     */
    fun onResponse(now: Long, code: Int, limit: Int?, remaining: Int?, resetAtMs: Long?, retryAfterMs: Long?): Long {
        limit?.takeIf { it > 0 }?.let { this.limit = it.coerceAtMost(MAX_LIMIT) }
        if (remaining != null) {
            this.remaining = remaining
            remainingAtMs = now
        }
        if (resetAtMs != null) this.resetAtMs = resetAtMs
        if (code != 429) return 0
        val wait = retryAfterMs ?: resetAtMs?.let { it - now }?.takeIf { it > 0 } ?: default429Ms
        cooldownUntilMs = maxOf(cooldownUntilMs, now + wait)
        last429AtMs = now
        // the cool-down covers the wait; a stale "remaining 0" must not block past it
        this.remaining = null
        return wait
    }

    companion object {
        const val DEFAULT_LIMIT = 30
        const val WINDOW_MS = 60_000L
        const val BACKGROUND_RESERVE = 3
        const val DEFAULT_429_MS = 60_000L
        private const val MAX_LIMIT = 1000
    }
}

/** Retry policy of a 429 (pure): retry once, transparently, when the wait is short; otherwise surface it with the wait. */
object AniListRetryPolicy {
    const val SHORT_WAIT_MS = 10_000L

    fun shouldRetry(attempt: Int, waitMs: Long, shortWaitMs: Long = SHORT_WAIT_MS): Boolean = attempt == 0 && waitMs <= shortWaitMs

    /** `Retry-After` value (whole seconds, rounded up, at least 1) for a wait. */
    fun retryAfterSeconds(waitMs: Long): Long = ((waitMs + 999) / 1000).coerceAtLeast(1)
}
