// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave A (15 §5 p (g)): sleep timer, subtitle delay arithmetic, the Media3 cue timeline and the font name reader.
class PlayerTimersTest {
    @Test
    fun sleepTimerFiresOnceAfterTheDeadline() {
        val t = SleepTimer()
        t.set(SleepOption.MIN_15, nowMs = 1_000)
        assertEquals(15 * 60_000L, t.remainingMs(1_000))
        assertFalse(t.tick(1_000 + 15 * 60_000L - 1))
        assertTrue(t.tick(1_000 + 15 * 60_000L))
        assertFalse(t.tick(1_000 + 16 * 60_000L))
        assertTrue(t.fired)
        assertFalse(t.active)
        assertNull(t.remainingMs(0))
    }

    @Test
    fun endOfEpisodeTimer() {
        val t = SleepTimer()
        assertFalse(t.onEpisodeEnded())
        t.set(SleepOption.END_OF_EPISODE, 0)
        assertNull(t.remainingMs(0))
        assertTrue(t.onEpisodeEnded())
        assertFalse(t.onEpisodeEnded())
    }

    @Test
    fun subtitleDelaySteps() {
        assertEquals(100L, SubtitleDelay.step(0, 1))
        assertEquals(-200L, SubtitleDelay.step(-100, -1))
        assertEquals(SubtitleDelay.MAX_MS, SubtitleDelay.step(SubtitleDelay.MAX_MS, 1))
        assertEquals("+0.3 s", SubtitleDelay.format(300))
        assertEquals("-1.2 s", SubtitleDelay.format(-1_200))
        assertEquals("0.0 s", SubtitleDelay.format(0))
        assertEquals(0.5, SubtitleDelay.seconds(500))
        assertEquals(9_500L, SubtitleDelay.lookupPositionMs(10_000, 500))
        assertEquals(10_500L, SubtitleDelay.lookupPositionMs(10_000, -500))
        assertEquals(0L, SubtitleDelay.lookupPositionMs(100, 500))
    }

    @Test
    fun cueTimelineShiftsAndForgetsStaleLines() {
        val t = CueTimeline()
        t.record(1_000, listOf("Hello"))
        t.record(3_000, emptyList())
        assertEquals(listOf("Hello"), t.linesAt(2_000))
        assertTrue(t.linesAt(3_500).isEmpty())
        assertTrue(t.linesAt(500).isEmpty())
        t.record(4_000, listOf("Late"))
        assertTrue(t.linesAt(4_000 + CueTimeline.MAX_GAP_MS + 1).isEmpty())
    }

    @Test
    fun subtitleExtrasSanitiseAndMpvColours() {
        val e = SubtitleExtras(outlineSize = 99f, positionPct = 140).sanitized()
        assertEquals(SubtitleExtras.MAX_OUTLINE, e.outlineSize)
        assertEquals(100, e.positionPct)
        assertEquals("1.000/1.000/1.000/1.000", SubtitleExtras.mpvColor(SubtitleExtras.WHITE))
        assertEquals("0.000/0.000/0.000/0.500", SubtitleExtras.mpvColor(SubtitleExtras.BLACK, 0.5f))
    }

    @Test
    fun fontFamilyFromTheNameTable() {
        val family = "Tatakai Sans"
        val utf16 = family.toByteArray(Charsets.UTF_16BE)
        // Offset table (1 table) + table record "name" + name table with one Windows record (name id 1).
        val nameOffset = 12 + 16
        val buf = java.nio.ByteBuffer.allocate(nameOffset + 6 + 12 + utf16.size)
        buf.putInt(0x00010000).putShort(1).putShort(0).putShort(0).putShort(0)
        buf.put("name".toByteArray(Charsets.US_ASCII)).putInt(0).putInt(nameOffset).putInt(6 + 12 + utf16.size)
        buf.putShort(0).putShort(1).putShort((6 + 12).toShort())
        buf.putShort(3).putShort(1).putShort(0x409).putShort(1).putShort(utf16.size.toShort()).putShort(0)
        buf.put(utf16)
        assertEquals(family, FontNames.family(buf.array()))
        assertNull(FontNames.family(ByteArray(8)))
        assertTrue(SubtitleFonts.isFontName("x.OTF"))
        assertFalse(SubtitleFonts.isFontName("x.png"))
    }
}
