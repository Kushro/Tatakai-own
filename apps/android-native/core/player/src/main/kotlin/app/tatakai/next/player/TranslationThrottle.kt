// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlinx.coroutines.delay
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.concurrent.ConcurrentHashMap
import kotlin.math.min
import kotlin.random.Random

// 0.4.0 (sub-translation worker): request pacing for the translation providers. The keyless Google endpoints answer HTTP 429 after a
// short burst (the old code sent one request per visible line, so a busy scene got the IP blocked for minutes). Every provider call
// now goes through a [RequestGate]: a GCRA token bucket (burst + steady interval), plus a circuit breaker that honours `Retry-After`
// or backs off exponentially with jitter after 429 / 503, and fails fast while it is open so the screen keeps the original lines.

/** Monotonic milliseconds; injectable so tests run on virtual time. */
fun interface MonotonicClock {
    fun nowMs(): Long

    companion object {
        val SYSTEM = MonotonicClock { System.nanoTime() / 1_000_000 }
    }
}

/**
 * One provider endpoint's pacing and back-off state. Thread-safe; share one per endpoint host for the whole process ([shared]).
 *
 * - [acquire] waits for the next slot of the bucket ([burst] requests at once, then one every [intervalMs]) and throws
 *   [TranslationException.Kind.RATE_LIMITED] at once while the breaker is open (callers must not queue behind a long ban).
 * - [onRateLimited] opens the breaker for `Retry-After` when the server sent one, else for [Backoff.delayMs] of the consecutive strike.
 * - [onSuccess] closes it and resets the strikes.
 */
class RequestGate(
    val burst: Int,
    val intervalMs: Long,
    private val clock: MonotonicClock = MonotonicClock.SYSTEM,
    private val backoff: Backoff = Backoff(),
) {
    private val mutex = Mutex()

    /** GCRA "theoretical arrival time" of the next request. */
    private var tat = Long.MIN_VALUE
    private var openUntil = Long.MIN_VALUE
    private var strikes = 0

    /** Milliseconds until the breaker closes (0 = closed). */
    fun cooldownMs(): Long = if (openUntil == Long.MIN_VALUE) 0 else (openUntil - clock.nowMs()).coerceAtLeast(0)

    /** Consecutive 429 / 503 answers since the last success. */
    fun strikes(): Int = strikes

    suspend fun acquire() {
        val wait = mutex.withLock {
            val now = clock.nowMs()
            if (openUntil > now) throw TranslationException.rateLimited(openUntil - now)
            val base = if (tat == Long.MIN_VALUE) now else maxOf(tat, now)
            val allowedAt = base - (burst - 1).coerceAtLeast(0) * intervalMs
            tat = base + intervalMs
            (allowedAt - now).coerceAtLeast(0)
        }
        if (wait > 0) delay(wait)
    }

    /** The server said "too many requests"; returns how long the breaker stays open. */
    suspend fun onRateLimited(retryAfterMs: Long?): Long = mutex.withLock {
        strikes++
        val wait = (retryAfterMs?.takeIf { it > 0 } ?: backoff.delayMs(strikes)).coerceAtMost(backoff.maxMs * 5)
        openUntil = maxOf(openUntil, clock.nowMs() + wait)
        wait
    }

    suspend fun onSuccess() = mutex.withLock {
        strikes = 0
        openUntil = Long.MIN_VALUE
    }

    companion object {
        private val gates = ConcurrentHashMap<String, RequestGate>()

        /** Process-wide gate of [key] (an endpoint host): every player / settings screen shares the same budget. */
        fun shared(key: String, create: () -> RequestGate): RequestGate = gates.getOrPut(key, create)

        /**
         * Keyless Google (`translate_a/single?client=gtx`, `translate_a/t?client=dict-chrome-ex`). No public quota exists; desktop
         * tools that batch like this report blocks after sustained > ~1 request/s, so: 2 at once, then one every 1.5 s.
         */
        fun googleFree(clock: MonotonicClock = MonotonicClock.SYSTEM) = RequestGate(burst = 2, intervalMs = 1_500, clock = clock)

        /** Keyed providers have real quotas; the gate only spaces bursts a little and honours their 429s. */
        fun keyed(clock: MonotonicClock = MonotonicClock.SYSTEM) = RequestGate(burst = 4, intervalMs = 250, clock = clock)
    }
}

/** Exponential back-off with jitter: 2 s, 4 s, 8 s ... capped at [maxMs], each ±[jitter] (so many clients do not retry in lockstep). */
class Backoff(
    val baseMs: Long = 2_000,
    val maxMs: Long = 60_000,
    private val jitter: Double = 0.2,
    private val random: Random = Random.Default,
) {
    fun delayMs(attempt: Int): Long {
        val exp = baseMs * (1L shl (attempt - 1).coerceIn(0, 20))
        val capped = min(exp, maxMs)
        if (jitter <= 0.0) return capped
        val spread = (capped * jitter).toLong()
        return (capped - spread + random.nextLong(0, spread * 2 + 1)).coerceIn(baseMs / 2, maxMs)
    }
}

object RetryAfter {
    private const val MAX_MS = 10 * 60_000L

    /** `Retry-After: 120` or an HTTP-date; null when absent / unparsable. [nowEpochMs] is wall-clock (dates are absolute). */
    fun parseMs(header: String?, nowEpochMs: Long = System.currentTimeMillis()): Long? {
        val v = header?.trim()?.takeIf { it.isNotEmpty() } ?: return null
        v.toLongOrNull()?.let { return (it * 1000).coerceIn(0, MAX_MS) }
        v.toDoubleOrNull()?.let { return (it * 1000).toLong().coerceIn(0, MAX_MS) }
        val date = runCatching { ZonedDateTime.parse(v, DateTimeFormatter.RFC_1123_DATE_TIME) }.getOrNull() ?: return null
        return (date.toInstant().toEpochMilli() - nowEpochMs).coerceIn(0, MAX_MS)
    }
}
