// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import kotlinx.coroutines.test.runTest
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// 0.4.0 (sub-translation worker): ASS text clean-up, per-event lookup of mpv's joined sub-text, and look-ahead batching.
class SubtitleLookaheadTest {
    @Test
    fun assTagsDrawingsAndKaraoke() {
        assertEquals("Hello\nworld", SubtitleText.fromAss("{\\an8}{\\i1}Hello{\\i0}\\Nworld").plain)
        assertEquals("No break here", SubtitleText.fromAss("No\\hbreak  here").plain)
        val drawing = SubtitleText.fromAss("{\\pos(10,10)\\p1}m 0 0 l 100 0 100 100 0 100{\\p0}")
        assertTrue(drawing.drawingOnly)
        assertEquals("", drawing.plain)
        // Text around a drawing survives; only the vector commands go.
        assertEquals("Sign", SubtitleText.fromAss("{\\p1}m 0 0 l 10 0{\\p0}Sign").plain)
        val kara = SubtitleText.fromAss("{\\k20}Ka{\\k30}ra{\\kf40}oke")
        assertTrue(kara.karaoke)
        assertEquals("Karaoke", kara.plain)
        assertTrue(SubtitleText.isDrawingLike("m 0 0 l 100 0 100 100 0 100"))
        assertTrue(SubtitleText.isDrawingLike("m 12.5 -3 b 1 2 3 4 5 6"))
        assertFalse(SubtitleText.isDrawingLike("me too"))
        assertFalse(SubtitleText.isDrawingLike("m 1"))
        assertEquals("a b\nc", SubtitleText.normalize("  a  b \n\n c\r\nm 0 0 l 100 0 100 100"))
    }

    @Test
    fun sidecarAssParserDropsDrawings() {
        val ass = "[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\n" +
            "Dialogue: 0,0:00:01.00,0:00:02.00,Sign,,0,0,0,,{\\p1}m 0 0 l 10 0 10 10{\\p0}\n" +
            "Dialogue: 0,0:00:01.00,0:00:02.00,Default,,0,0,0,,{\\i1}Hi{\\i0}\n"
        assertEquals(listOf("Hi"), SubtitleParser.parse(ass).map { it.text })
    }

    @Test
    fun lookupRebuildsJoinedEvents() {
        val tr = mapOf("Hello" to "Hola", "How are you?\nFine" to "¿Cómo estás?\nBien")
        assertEquals("Hola", SubtitleText.lookup(tr, " Hello "))
        assertEquals("Hola\n¿Cómo estás?\nBien", SubtitleText.lookup(tr, "Hello\nHow are you?\nFine"))
        assertEquals("Hola\nUnknown", SubtitleText.lookup(tr, "Hello\nUnknown"))
        assertNull(SubtitleText.lookup(tr, "Nothing\nKnown"))
        assertEquals(listOf("Unknown"), SubtitleText.missingParts(tr, "Hello\nUnknown"))
        assertEquals(emptyList(), SubtitleText.missingParts(tr, "Hello\nHow are you?\nFine"))
        assertEquals(listOf("Two\nlines"), SubtitleText.missingParts(tr, "Two\nlines"))
    }

    private fun cues(vararg c: Triple<Long, Long, String>) = c.map { TimedCue(it.first, it.second, it.third) }

    @Test
    fun upcomingAndBatchFilling() {
        val track = cues(
            Triple(0, 60_000, "Sign up top"), // long sign that started before the position
            Triple(10_000, 12_000, "a"),
            Triple(20_000, 22_000, "b"),
            Triple(400_000, 402_000, "far"),
            Triple(700_000, 702_000, "too far"),
        )
        val up = SubtitleTranslationSession.upcoming(track, 15_000, SubtitleTranslationSession.LOOKAHEAD_MAX_MS)
        assertEquals(listOf("Sign up top", "b", "far"), up.map { it.text })
        val batch = SubtitleTranslationSession.fillBatch(listOf("live"), up)
        assertEquals(listOf("live", "Sign up top", "b", "far"), batch)
        // Caps: never more than 40 lines / 3000 chars of look-ahead.
        val many = (1..100).map { TimedCue(it * 1_000L, it * 1_000L + 500, "line $it") }
        assertEquals(SubtitleTranslationSession.BATCH_MAX_LINES, SubtitleTranslationSession.fillBatch(emptyList(), many).size)
        val long = (1..10).map { TimedCue(it * 1_000L, it * 1_000L + 500, "x".repeat(1_000) + it) }
        assertEquals(2, SubtitleTranslationSession.fillBatch(emptyList(), long).size)
    }

    @Test
    fun liveRequestCarriesLookAheadAndAheadWaitsUntilDue() = runTest {
        val dir = Files.createTempDirectory("subs-lookahead").toFile()
        val batches = mutableListOf<List<String>>()
        val translator = SubtitleTranslator { lines -> batches += lines; lines.map { "T:$it" } }
        val session = SubtitleTranslationSession(translator, TranslationCache(dir), "google|es|show")
        val track = (0 until 120).map { i -> TimedCue(i * 5_000L, i * 5_000L + 2_000, "cue $i") }
        session.setTimeline(track, skipTexts = setOf("cue 3"))

        // One request: the line on screen + the next cues (not the skipped karaoke one).
        val got = session.translateNow(listOf("cue 0"), positionMs = 0)
        assertEquals(1, batches.size)
        assertEquals("cue 0", batches[0].first())
        assertFalse("cue 3" in batches[0])
        assertEquals(40, batches[0].size)
        assertEquals("T:cue 0", got["cue 0"])
        assertEquals("T:cue 4", got["cue 4"])

        // Everything up to ~cue 40 is cached: nothing due yet at 10 s (next untranslated starts at 205 s, beyond the 90 s trigger).
        assertTrue(session.translateAhead(10_000).isEmpty())
        assertEquals(1, batches.size)
        // At 150 s it is due: one batch starting at the first untranslated cue.
        val ahead = session.translateAhead(150_000)
        assertEquals(2, batches.size)
        assertEquals("cue 41", batches[1].first())
        assertTrue(ahead.isNotEmpty())
        // Cached lines are never requested again.
        session.translateNow(listOf("cue 0", "cue 41"), positionMs = 0)
        assertEquals(2, batches.size)
        dir.deleteRecursively()
    }

    @Test
    fun pretranslateWaitsOutRateLimits() = runTest {
        val dir = Files.createTempDirectory("subs-pre").toFile()
        var calls = 0
        val translator = SubtitleTranslator { lines ->
            calls++
            if (calls == 1) throw TranslationException.rateLimited(4_000)
            lines.map { "T:$it" }
        }
        val session = SubtitleTranslationSession(translator, TranslationCache(dir), "s")
        val waits = mutableListOf<Long>()
        val failed = session.pretranslate((1..50).map { "l$it" }, onThrottled = { waits += it }) { _, _ -> }
        assertEquals(0, failed)
        assertEquals(listOf(4_000L), waits)
        assertEquals(3, calls) // the throttled batch again + the second batch
        assertEquals(4_000, testScheduler.currentTime)
        dir.deleteRecursively()
    }

    @Test
    fun cacheIsSharedAcrossEpisodesOfASeries() = runTest {
        val dir = Files.createTempDirectory("subs-series").toFile()
        var calls = 0
        val translator = SubtitleTranslator { lines -> calls++; lines.map { "T:$it" } }
        val cfg = TranslatorConfig(app.tatakai.next.model.TranslationProvider.GOOGLE, "es")
        val scope = TranslationCache.scope("anilist:123", cfg)
        SubtitleTranslationSession(translator, TranslationCache(dir), scope).translateLive(listOf("Opening line"))
        // A new player (next episode, same series) reads it from disk.
        assertEquals(listOf("T:Opening line"), SubtitleTranslationSession(translator, TranslationCache(dir), scope).translateLive(listOf("Opening line")))
        assertEquals(1, calls)
        dir.deleteRecursively()
    }
}
