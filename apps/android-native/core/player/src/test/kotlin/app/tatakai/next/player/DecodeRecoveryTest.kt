// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlin.test.Test
import app.tatakai.next.model.Container
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

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

    @Test
    fun anMpvStartTimeoutMovesTheSourceToMedia3Once() {
        // BUG-059: the PC's anizone HLS (15 audio renditions, one without segments) never starts in libmpv.
        assertTrue(switchToMedia3OnStall(MPV_START_TIMEOUT_CODE, onMpv = true, remote = false, triedMedia3 = false))
        assertFalse(switchToMedia3OnStall(MPV_START_TIMEOUT_CODE, onMpv = true, remote = false, triedMedia3 = true), "only once per source")
        assertFalse(switchToMedia3OnStall(MPV_START_TIMEOUT_CODE, onMpv = true, remote = true, triedMedia3 = false), "Link already uses Media3")
        assertFalse(switchToMedia3OnStall(-1, onMpv = true, remote = false, triedMedia3 = false), "other mpv errors fail the source")
        assertFalse(switchToMedia3OnStall(MPV_START_TIMEOUT_CODE, onMpv = false, remote = false, triedMedia3 = false))
    }

    @Test
    fun anMpvOpenErrorBeforeTheFirstFrameAlsoMovesTheSourceToMedia3Once() {
        // BUG-080: "Protocol 'http' not on whitelist" fails the load in ~2 s, long before the start timeout.
        assertTrue(switchToMedia3OnStall(MPV_OPEN_ERROR_CODE, onMpv = true, remote = false, triedMedia3 = false))
        assertFalse(switchToMedia3OnStall(MPV_OPEN_ERROR_CODE, onMpv = true, remote = false, triedMedia3 = true), "only once per source")
        assertFalse(switchToMedia3OnStall(MPV_OPEN_ERROR_CODE, onMpv = true, remote = true, triedMedia3 = false), "Link already uses Media3")
        assertFalse(switchToMedia3OnStall(MPV_OPEN_ERROR_CODE, onMpv = false, remote = false, triedMedia3 = false))
    }

    @Test
    fun torrentStreamsAreNotWatchedByTheStartTimeout() {
        assertTrue(mpvStartWatched(Container.HLS, torrent = false))
        assertTrue(mpvStartWatched(Container.MP4, torrent = false))
        assertFalse(mpvStartWatched(Container.PROGRESSIVE, torrent = true))
        assertFalse(mpvStartWatched(Container.MP4, torrent = true))
    }
}
