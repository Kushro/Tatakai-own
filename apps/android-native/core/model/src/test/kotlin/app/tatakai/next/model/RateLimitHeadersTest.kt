// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.runBlocking
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class RateLimitHeadersTest {
    @Test
    fun parsesCountsAndReset() {
        assertEquals(90, RateLimitHeaders.count(" 90 "))
        assertNull(RateLimitHeaders.count("-1"))
        assertNull(RateLimitHeaders.count("x"))
        assertNull(RateLimitHeaders.count(null))
        assertEquals(1_700_000_000_000L, RateLimitHeaders.resetAtMs("1700000000"))
        assertNull(RateLimitHeaders.resetAtMs("0"))
    }

    @Test
    fun parsesRetryAfterSecondsAndDates() {
        assertEquals(30_000L, RateLimitHeaders.retryAfterMs("30", 0))
        assertEquals(0L, RateLimitHeaders.retryAfterMs("0", 0))
        assertNull(RateLimitHeaders.retryAfterMs("-5", 0))
        assertNull(RateLimitHeaders.retryAfterMs("", 0))
        assertNull(RateLimitHeaders.retryAfterMs("soon", 0))
        // 1970-01-01T00:01:40Z = 100 s
        assertEquals(40_000L, RateLimitHeaders.retryAfterMs("Thu, 01 Jan 1970 00:01:40 GMT", 60_000))
        assertEquals(0L, RateLimitHeaders.retryAfterMs("Thu, 01 Jan 1970 00:01:40 GMT", 200_000))
    }

    @Test
    fun priorityDefaultsToInteractive() = runBlocking {
        assertEquals(RequestPriority.INTERACTIVE, currentRequestPriority())
        assertEquals(RequestPriority.BACKGROUND, inBackground { currentRequestPriority() })
    }

    @Test
    fun retryAfterIsNotPartOfEquality() {
        assertEquals<AppError>(AppError.RateLimited("anilist"), AppError.RateLimited("anilist", 30_000))
        assertEquals(30_000L, AppError.RateLimited("anilist", 30_000).retryAfterMs)
    }
}
