// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.cancel
import kotlinx.coroutines.delay
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeout
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

/**
 * BUG-099 / BUG-100: quickjs-kt only ends an evaluation when no host job of it is still running, so a fetch or a timer a bundle gave
 * up on held the answer (Toko novels: ready in ~1 s, returned after ~80 s on the phone). Once the call settles, its jobs return at once.
 */
class HostCallGateTest {
    private fun scope() = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    @Test
    fun a_job_still_running_when_the_call_settles_returns_its_abandoned_value_at_once() = runBlocking {
        val scope = scope()
        val gate = HostCallGate(scope)
        gate.begin()
        val started = System.nanoTime()
        val slowFetch = async { gate.run("abandoned") { delay(30_000); "late answer" } }
        delay(50)
        gate.settle()
        assertEquals("abandoned", withTimeout(2_000) { slowFetch.await() })
        assertTrue((System.nanoTime() - started) / 1_000_000 < 2_000)
        scope.cancel()
    }

    @Test
    fun jobs_that_finish_before_the_answer_keep_their_result() = runBlocking {
        val scope = scope()
        val gate = HostCallGate(scope)
        gate.begin()
        assertEquals("body", gate.run("abandoned") { delay(10); "body" })
        gate.settle()
        // A job started after the answer does not even start; the next call runs normally again.
        assertEquals("abandoned", gate.run("abandoned") { "never" })
        gate.begin()
        assertEquals("again", gate.run("abandoned") { "again" })
        scope.cancel()
    }

    @Test
    fun a_failure_before_the_answer_still_reaches_the_bundle() = runBlocking {
        val scope = scope()
        val gate = HostCallGate(scope)
        gate.begin()
        val r = runCatching { gate.run("abandoned") { error("network down") } }
        assertEquals("network down", r.exceptionOrNull()?.message)
        scope.cancel()
    }
}
