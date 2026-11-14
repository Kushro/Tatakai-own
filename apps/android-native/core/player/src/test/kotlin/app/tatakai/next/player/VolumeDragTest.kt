// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

// BUG-044: the volume gesture starts from the live system volume, respects the mute and never jumps to the maximum.
class VolumeDragTest {
    @Test
    fun startsFromTheLiveVolumeAndMovesOneStepAtATime() {
        val d = VolumeDrag(startIndex = 0, maxIndex = 15, muted = false)
        // A tiny jitter (less than one step) changes nothing.
        assertNull(d.onDrag(0.02f))
        // One step needs 1.2 / 15 = 0.08 of the height.
        assertEquals(1, d.onDrag(0.07f))
        assertEquals(1, d.index)
        assertEquals(6, d.percent)
    }

    @Test
    fun aShortSwipeFromZeroNeverReachesTheMaximum() {
        val d = VolumeDrag(startIndex = 0, maxIndex = 15, muted = false)
        var last: Int? = null
        repeat(10) { d.onDrag(0.02f)?.let { last = it } } // a 20 % swipe
        assertEquals(2, last)
    }

    @Test
    fun mutedStreamIsNeverChanged() {
        val d = VolumeDrag(startIndex = 15, maxIndex = 15, muted = true)
        assertNull(d.onDrag(1f))
        assertNull(d.onDrag(-1f))
        assertEquals(0, d.percent)
    }

    @Test
    fun clampsAndGoesDown() {
        val d = VolumeDrag(startIndex = 10, maxIndex = 15, muted = false)
        assertEquals(15, d.onDrag(2f))
        assertNull(d.onDrag(0.5f))
        val e = VolumeDrag(startIndex = 3, maxIndex = 15, muted = false)
        assertEquals(0, e.onDrag(-1f))
        assertNull(e.onDrag(-1f))
    }
}
