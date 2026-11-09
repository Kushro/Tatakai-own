// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class SubtitleParserTest {
    @Test
    fun parsesSrt() {
        val srt = "1\r\n00:00:01,000 --> 00:00:02,500\r\n<i>Hello</i>\r\nthere\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nBye\r\n"
        val cues = SubtitleParser.parse(srt)
        assertEquals(listOf(TimedCue(1_000, 2_500, "Hello\nthere"), TimedCue(3_000, 4_000, "Bye")), cues)
        assertEquals(listOf("Hello\nthere"), SubtitleParser.activeAt(cues, 2_000))
        assertTrue(SubtitleParser.activeAt(cues, 2_600).isEmpty())
    }

    @Test
    fun parsesVttAndAss() {
        val vtt = "WEBVTT\n\n00:01.000 --> 00:02.000 align:start\n<c.yellow>Hi</c>\n"
        assertEquals(listOf(TimedCue(1_000, 2_000, "Hi")), SubtitleParser.parse(vtt))
        val ass = "[Script Info]\nTitle: x\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n" +
            "Dialogue: 0,0:00:05.00,0:00:06.50,Default,,0,0,0,,{\\i1}One, two\\Nthree\n"
        assertEquals(listOf(TimedCue(5_000, 6_500, "One, two\nthree")), SubtitleParser.parse(ass))
        assertEquals("ass", SubtitleParser.formatOf("ep1.SSA"))
        assertEquals("vtt", SubtitleParser.formatOf("https://x/sub.vtt?token=1"))
    }

    @Test
    fun uniqueTextsForPretranslation() {
        val cues = listOf(TimedCue(0, 1, "a"), TimedCue(1, 2, "b"), TimedCue(2, 3, "a"))
        assertEquals(listOf("a", "b"), SubtitleParser.uniqueTexts(cues))
    }
}
