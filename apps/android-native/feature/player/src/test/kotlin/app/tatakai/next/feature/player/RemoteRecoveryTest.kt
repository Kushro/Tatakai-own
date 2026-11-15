// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import kotlin.test.Test
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class RemoteRecoveryTest {
    @Test
    fun aClosedSessionIsAlwaysRenewed() {
        assertTrue(shouldRenewRemote(alive = false, canRenew = true, auto = true))
        assertTrue(shouldRenewRemote(alive = false, canRenew = true, auto = false))
    }

    @Test
    fun retryOnAStuckSessionOpensANewOne() {
        // BUG-009 / BUG-023: Reintentar reused the stuck session and failed with ParserException
        assertTrue(shouldRenewRemote(alive = true, canRenew = true, auto = false))
        // the automatic path only shows the error
        assertFalse(shouldRenewRemote(alive = true, canRenew = true, auto = true))
    }

    @Test
    fun noRenewWithoutTheSourceOrWithoutThePc() {
        assertFalse(shouldRenewRemote(alive = false, canRenew = false, auto = false))
        assertFalse(shouldRenewRemote(alive = null, canRenew = true, auto = false))
        assertFalse(shouldRenewRemote(alive = null, canRenew = true, auto = true))
    }

    @Test
    fun anExpiredHandOffIsRenewedBeforeItLoads() {
        // BUG-138: the PC closed the hand-off session (120 s idle) before "Reproducir aquí": a new one at once, no 65 s of 410s
        assertTrue(renewRemoteBeforeLoad(alive = false, canRenew = true))
        // still alive, or the PC could not be asked (network cut): load it as before, the retries / "connection lost" handle it
        assertFalse(renewRemoteBeforeLoad(alive = true, canRenew = true))
        assertFalse(renewRemoteBeforeLoad(alive = null, canRenew = true))
        // nothing to renew it from: loading it fails at once (404 / 410 are not retried) and says "send it again"
        assertFalse(renewRemoteBeforeLoad(alive = false, canRenew = false))
    }
}
