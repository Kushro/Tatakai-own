// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.TimeoutCancellationException
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlin.test.Test
import kotlin.test.assertEquals

/** BUG-099/100: a card without chapters says why. */
class SourceProblemTest {
    @Test
    fun timeouts_and_failures_are_told_apart() {
        assertEquals(SourceProblem.TIMEOUT, SourceProblem.of(AppException(AppError.Extension("tatakai.extension.toko", "timeout"))))
        assertEquals(SourceProblem.FAILED, SourceProblem.of(AppException(AppError.Extension("tatakai.extension.toko", "Extension disabled"))))
        assertEquals(SourceProblem.FAILED, SourceProblem.of(IllegalStateException("boom")))
        val timeout = runBlocking { runCatching { withTimeout(1) { delay(1_000) } }.exceptionOrNull() }
        assertEquals(true, timeout is TimeoutCancellationException)
        assertEquals(SourceProblem.TIMEOUT, SourceProblem.of(timeout))
    }

    /** BUG-125: "HentaiDB · 80%" without a reason: an own source that did not find the title now says why. */
    @Test
    fun anOwnSourceSaysWhyItHasNoChapters() {
        assertEquals(null, SourceProblem.ofSearch(found = true, timedOut = false, searches = 2, failed = 0))
        assertEquals(SourceProblem.NOT_FOUND, SourceProblem.ofSearch(found = false, timedOut = false, searches = 2, failed = 1))
        assertEquals(SourceProblem.NOT_FOUND, SourceProblem.ofSearch(found = false, timedOut = false, searches = 0, failed = 0))
        assertEquals(SourceProblem.TIMEOUT, SourceProblem.ofSearch(found = false, timedOut = true, searches = 1, failed = 0))
        assertEquals(SourceProblem.FAILED, SourceProblem.ofSearch(found = false, timedOut = false, searches = 2, failed = 2))
    }
}
