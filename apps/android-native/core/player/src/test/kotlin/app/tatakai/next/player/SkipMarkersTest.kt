// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.SkipKind
import app.tatakai.next.model.SkipMark
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

// Ported from the desktop tests of tk-ui/src/player_state.rs (Markers, range helpers).
class SkipMarkersTest {
    private val marks = listOf(
        SkipMark(SkipKind.INTRO, 90.0, 180.0),
        SkipMark(SkipKind.INTRO, 30.0, 60.0),
        SkipMark(SkipKind.OUTRO, 1300.0, 1390.0),
        SkipMark(SkipKind.RECAP, 0.0, 20.0),
        SkipMark(SkipKind.OUTRO, 50.0, 10.0),
    )

    @Test
    fun earliestMarkOfEachKindWinsAndMalformedOnesAreSkipped() {
        val m = Markers.from(marks)
        assertEquals(Span(30.0, 60.0), m.intro)
        assertEquals(Span(1300.0, 1390.0), m.outro)
        assertEquals(Span(0.0, 20.0), m.recap)
        assertTrue(Markers.from(emptyList()).isEmpty)
    }

    @Test
    fun skipTargetsFollowThePosition() {
        val m = Markers.from(marks)
        assertEquals(SkipKind.RECAP to 20.0, m.skipAt(5.0))
        assertEquals(SkipKind.INTRO to 60.0, m.skipAt(30.0))
        assertNull(m.skipAt(60.0), "end is exclusive")
        assertEquals(60.0, m.skipTarget(45.0))
        assertNull(m.skipTarget(5.0), "skip intro only inside the intro")
        assertEquals(SkipKind.OUTRO to 1390.0, m.skipAt(1350.0))
    }

    @Test
    fun barMarksAreFractionsInRecapIntroOutroOrder() {
        val m = Markers.from(marks)
        val bars = m.barMarks(1400.0)
        assertEquals(listOf(SkipKind.RECAP, SkipKind.INTRO, SkipKind.OUTRO), bars.map { it.kind })
        assertEquals(30f / 1400f, bars[1].left, 1e-6f)
        assertEquals(30f / 1400f, bars[1].width, 1e-6f)
        assertTrue(m.barMarks(0.0).isEmpty())
        assertTrue(m.barMarks(Double.NaN).isEmpty())
        assertEquals(2, m.barMarks(100.0).size, "marks after the end are dropped")
    }

    @Test
    fun rangeHelpers() {
        val ranges = listOf(0.0..0.25, 0.5..0.6, 0.9..1.4)
        assertEquals(listOf(0f to 0.25f, 0.5f to 0.1f, 0.9f to 0.1f).map { it.first }, rangeSpans(ranges).map { it.first })
        assertEquals(0.45, downloadedFraction(ranges), 1e-9)
        assertEquals(0.1f to 0.15f, readySpan(ranges, 0.1)?.let { it.first to (Math.round(it.second * 100) / 100f) })
        assertNull(readySpan(ranges, 0.3), "not downloaded there yet")
        assertTrue(rangeSpans(listOf(0.4..0.4)).isEmpty())
    }

    @Test
    fun infoHashesAndCountdown() {
        val hash = "0123456789abcdef0123456789ABCDEF01234567"
        assertTrue(isInfoHash(hash))
        assertEquals(hash.lowercase(), magnetInfoHash("magnet:?xt=urn:btih:$hash&dn=x"))
        assertEquals(hash.lowercase(), magnetInfoHash(hash))
        assertNull(magnetInfoHash("magnet:?xt=urn:btih:ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"))
        assertEquals(10, countdownLeft(1_000, 1_000))
        assertEquals(7, countdownLeft(1_000, 4_500))
        assertEquals(0, countdownLeft(0, 60_000))
    }
}
