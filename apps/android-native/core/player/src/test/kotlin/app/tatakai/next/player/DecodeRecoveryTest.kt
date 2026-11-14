// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlin.test.Test
import kotlin.test.assertEquals

class DecodeRecoveryTest {
    @Test
    fun hevcTenBitWithoutHardwareGoesStraightToMpv() {
        // BUG-020: Judas HEVC 10 bits on the moto g51 under Media3 (4003).
        assertEquals(DecodeRecovery.SWITCH_TO_MPV, decodeRecovery(4003, onMedia3 = true, mpvAvailable = true, hardwareDecodable = false, silentRetries = 0))
    }

    @Test
    fun aTransientErrorOnAHardwareVideoIsRetriedSilentlyOnceThenMpv() {
        // BUG-042 (a): H.264 HLS, decoder error right after a double-tap seek.
        assertEquals(DecodeRecovery.RETRY_SAME, decodeRecovery(4003, true, true, true, 0))
        assertEquals(DecodeRecovery.SWITCH_TO_MPV, decodeRecovery(4003, true, true, true, 1))
        assertEquals(DecodeRecovery.NONE, decodeRecovery(4003, true, false, true, 1))
    }

    @Test
    fun withoutMpvTheSourceStillGetsOneRetry() {
        assertEquals(DecodeRecovery.RETRY_SAME, decodeRecovery(4001, true, false, false, 0))
        assertEquals(DecodeRecovery.NONE, decodeRecovery(4001, true, false, false, 1))
    }

    @Test
    fun otherErrorsAndMpvErrorsAreLeftToTheSourceFlow() {
        assertEquals(DecodeRecovery.NONE, decodeRecovery(2001, true, true, true, 0), "network errors are not decoder errors")
        assertEquals(DecodeRecovery.NONE, decodeRecovery(-1, false, true, false, 0))
    }
}
