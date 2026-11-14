// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.async
import kotlinx.coroutines.selects.select

/**
 * BUG-099 / BUG-100: the top-level `QuickJs.evaluate` of quickjs-kt 1.0.15 only returns once the result promise is settled AND no
 * async host job of the evaluation is still active. A bundle that gives up on a slow provider (Toko: 8 s for manga/novel
 * discovery) leaves its fetches (OkHttp, up to 30 s), its cleared timers (each `__host_sleep` keeps running, up to 15 s) and its
 * hidden-browser operations (a Cloudflare solve, up to 90 s) behind, and the call only answered when the slowest of them ended:
 * ~80 s for "Mushoku Tensei", well past the callers' timeouts, so the phone showed nothing while the PC (rquickjs, which answers as
 * soon as the promise settles) listed chapters.
 *
 * The gate ends that: the prelude's `__tk_call` calls `__host_settled` as soon as the bundle's promise settles ([settle]); from then
 * on every host job still running for the call returns its `abandoned` value at once (the work itself is cancelled in [scope] and
 * nobody waits for it), so the evaluation returns right away, like the desktop host. The JS side has already dropped its timers.
 */
internal class HostCallGate(private val scope: CoroutineScope) {
    @Volatile private var done = CompletableDeferred<Unit>()

    /** A new call starts: host jobs it launches run until [settle]. */
    fun begin() {
        done = CompletableDeferred()
    }

    /** The call's result is known: the host jobs still running for it stop waiting. */
    fun settle() {
        done.complete(Unit)
    }

    val settled: Boolean get() = done.isCompleted

    /** Runs [block] for the current call; once the call settles it returns [abandoned] instead (and [block] is cancelled). */
    suspend fun <T> run(abandoned: T, block: suspend () -> T): T {
        val call = done
        if (call.isCompleted) return abandoned
        val work = scope.async { block() }
        try {
            return select {
                work.onAwait { it }
                call.onAwait { abandoned }
            }
        } finally {
            if (!work.isCompleted) work.cancel()
        }
    }
}
