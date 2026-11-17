// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import kotlin.test.Test
import kotlin.test.assertEquals

/** 0.4.1 (seek bar): `demuxer-cache-time` is where the cache ends, not how much is cached. */
class MpvBufferedTest {
    @Test
    fun theCacheEndIsTheBufferedPosition() {
        // 10 min in, 40 s cached ahead: mpv reports 640 s (the 0.4.0 bar drew 1250 s)
        assertEquals(640_000L, mpvBufferedMs(positionSecs = 600.0, cacheEndSecs = 640.0))
    }

    @Test
    fun neverBehindThePlayhead() {
        // right after a seek forward the property still holds the old end until the cache refills
        assertEquals(900_000L, mpvBufferedMs(positionSecs = 900.0, cacheEndSecs = 640.0))
        assertEquals(0L, mpvBufferedMs(positionSecs = 0.0, cacheEndSecs = 0.0))
    }
}
