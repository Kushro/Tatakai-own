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
}
