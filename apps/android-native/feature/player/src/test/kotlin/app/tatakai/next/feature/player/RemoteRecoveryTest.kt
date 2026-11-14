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
}
