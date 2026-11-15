// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.network

import app.tatakai.next.model.RequestPriority.BACKGROUND
import app.tatakai.next.model.RequestPriority.INTERACTIVE
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class AniListLimiterTest {
    @Test
    fun startsAtThirtyPerMinuteInASlidingWindow() {
        val l = AniListLimiter()
        repeat(30) { i -> assertEquals(0L, l.tryAcquire(1_000L + i * 100, INTERACTIVE)) }
        // the 31st waits until the first send (t = 1000) leaves the 60 s window
        assertEquals(1_000L + 60_000 - 5_000, l.waitMs(5_000, INTERACTIVE))
        assertEquals(30, l.sentInWindow(5_000))
        assertEquals(0L, l.tryAcquire(61_000, INTERACTIVE))
    }

    @Test
    fun backgroundLeavesAReserveForInteractive() {
        val l = AniListLimiter(defaultLimit = 10, backgroundReserve = 3)
        repeat(7) { assertEquals(0L, l.tryAcquire(0, BACKGROUND)) }
        assertTrue(l.waitMs(0, BACKGROUND) > 0)
        assertEquals(0L, l.tryAcquire(0, INTERACTIVE))
    }

    @Test
    fun learnsTheLimitFromTheHeaders() {
        val l = AniListLimiter()
        l.onResponse(now = 0, code = 200, limit = 90, remaining = 89, resetAtMs = null, retryAfterMs = null)
        assertEquals(90, l.limit)
        repeat(60) { assertEquals(0L, l.tryAcquire(1, INTERACTIVE)) }
    }

    @Test
    fun lowRemainingWaitsUntilReset() {
        val l = AniListLimiter()
        l.tryAcquire(0, INTERACTIVE)
        l.onResponse(now = 10, code = 200, limit = 30, remaining = 0, resetAtMs = 20_000, retryAfterMs = null)
        assertEquals(19_990L, l.waitMs(10, INTERACTIVE))
        // background keeps its reserve: remaining 2 already blocks it
        l.onResponse(now = 25_000, code = 200, limit = 30, remaining = 2, resetAtMs = 40_000, retryAfterMs = null)
        assertEquals(0L, l.waitMs(25_000, INTERACTIVE))
        assertEquals(15_000L, l.waitMs(25_000, BACKGROUND))
    }

    @Test
    fun lowRemainingWithoutResetWaitsForTheWindow() {
        val l = AniListLimiter()
        l.tryAcquire(1_000, INTERACTIVE)
        l.onResponse(now = 1_100, code = 200, limit = 30, remaining = 0, resetAtMs = null, retryAfterMs = null)
        assertEquals(1_000L + 60_000 - 2_000, l.waitMs(2_000, INTERACTIVE))
    }

    @Test
    fun staleRemainingIsIgnored() {
        val l = AniListLimiter()
        l.onResponse(now = 0, code = 200, limit = 30, remaining = 0, resetAtMs = null, retryAfterMs = null)
        assertEquals(0L, l.waitMs(61_000, INTERACTIVE))
    }

    @Test
    fun a429CoolsDownForRetryAfterThenResetThenDefault() {
        val l = AniListLimiter()
        assertEquals(30_000L, l.onResponse(now = 0, code = 429, limit = null, remaining = null, resetAtMs = 90_000, retryAfterMs = 30_000))
        assertEquals(30_000L, l.waitMs(0, INTERACTIVE))
        assertEquals(0L, l.last429AtMs)

        val r = AniListLimiter()
        assertEquals(45_000L, r.onResponse(now = 5_000, code = 429, limit = null, remaining = null, resetAtMs = 50_000, retryAfterMs = null))

        val d = AniListLimiter()
        assertEquals(AniListLimiter.DEFAULT_429_MS, d.onResponse(now = 0, code = 429, limit = null, remaining = null, resetAtMs = null, retryAfterMs = null))
        assertEquals(0L, d.onResponse(now = 0, code = 200, limit = null, remaining = null, resetAtMs = null, retryAfterMs = null))
    }

    @Test
    fun retryPolicyRetriesOnceOnlyForShortWaits() {
        assertTrue(AniListRetryPolicy.shouldRetry(attempt = 0, waitMs = 2_000))
        assertTrue(AniListRetryPolicy.shouldRetry(attempt = 0, waitMs = 10_000))
        assertFalse(AniListRetryPolicy.shouldRetry(attempt = 0, waitMs = 10_001))
        assertFalse(AniListRetryPolicy.shouldRetry(attempt = 1, waitMs = 1_000))
        assertEquals(1L, AniListRetryPolicy.retryAfterSeconds(1))
        assertEquals(31L, AniListRetryPolicy.retryAfterSeconds(30_001))
    }
}
