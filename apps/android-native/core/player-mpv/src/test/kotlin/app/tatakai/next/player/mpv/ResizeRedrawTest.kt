// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import kotlin.test.Test

class ResizeRedrawTest {
    @Test
    fun onlyAPausedFrameIsRedrawnByHand() {
        // BUG-093: paused libmpv, landscape -> portrait: stretched frame with a black band until resume.
        kotlin.test.assertTrue(needsRedrawAfterResize(paused = true, started = true, ended = false))
        kotlin.test.assertFalse(needsRedrawAfterResize(paused = false, started = true, ended = false), "playing: the next frame redraws")
        kotlin.test.assertFalse(needsRedrawAfterResize(paused = true, started = false, ended = false), "nothing shown yet")
        kotlin.test.assertFalse(needsRedrawAfterResize(paused = true, started = true, ended = true))
    }
}
