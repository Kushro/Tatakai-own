// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.NovelChapter
import app.tatakai.next.model.NovelReaderSettings
import app.tatakai.next.model.TtsEngine
import app.tatakai.next.model.TtsSettings
import kotlinx.coroutines.test.runTest
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import java.net.InetAddress

class NovelLogicTest {
    @get:Rule
    val tmp = TemporaryFolder()

    @Test
    fun htmlCleanupDropsNoiseAndKeepsParagraphs() {
        val html = """
            <html><body><nav>Home</nav><div id="chapter-content">
            <p>First   line.</p><script>alert(1)</script><div class="ads">BUY</div>
            <p>&nbsp;</p><p>Second line.</p><p>Next Chapter</p></div></body></html>
        """.trimIndent()
        assertEquals(listOf("First line.", "Second line."), NovelText.paragraphsFromHtml(html))
        assertEquals(listOf("One", "Two", "Three"), NovelText.paragraphsFromHtml("<div>One<br>Two<br/>Three</div>"))
    }

    @Test
    fun plainTextSplitting() {
        assertEquals(listOf("Para one continues.", "Para two."), NovelText.splitPlain("Para one\ncontinues.\n\n  Para two.  "))
        assertEquals(listOf("A", "B"), NovelText.splitPlain("A\nB\n"))
        assertTrue(NovelText.looksLikeHtml("<p>x</p>"))
        assertFalse(NovelText.looksLikeHtml("a < b"))
        assertTrue(NovelText.looksLikeUrl("https://x.org/ch/1"))
        assertFalse(NovelText.looksLikeUrl("https://x.org is great"))
    }

    @Test
    fun pagesFromExtension() = runTest {
        val pages = listOf(
            ChapterPage(1, "<p>B1</p><p>B2</p>"),
            ChapterPage(0, "Plain A"),
            ChapterPage(2, "https://site/ch2"),
        )
        val out = NovelText.fromPages(pages) { page -> assertEquals("https://site/ch2", page.url); "<p>C1</p><p>C2</p>" }
        assertEquals(listOf("Plain A", "B1", "B2", "C1", "C2"), out)
    }

    @Test
    fun speechChunksRespectLimit() {
        val sentence = "This is a sentence. "
        val long = sentence.repeat(300)
        val chunks = NovelText.speechChunks(long, max = 500)
        assertTrue(chunks.all { it.length <= 500 })
        assertTrue(chunks.all { it.endsWith(".") })
        assertEquals(long.trim().replace(" ", ""), chunks.joinToString("").replace(" ", ""))
        assertEquals(listOf("short"), NovelText.speechChunks(" short "))
        assertEquals(emptyList<String>(), NovelText.speechChunks("   "))
        assertEquals(4, NovelText.wordCount(listOf("a b", "c  d")))
    }

    private fun nch(id: String, n: Double?, tr: String? = null, lang: String? = "en") =
        NovelChapter(id = id, extensionId = "e", sourceId = "s", itemId = "i", number = n, title = "Chapter ${n ?: "?"}", lang = lang, translator = tr)

    @Test
    fun novelChapterHelpers() {
        val list = listOf(nch("2", 2.0, "TL A"), nch("1", 1.0, "TL B"), nch("1b", 1.0, "TL A", "es"), nch("x", null))
        assertEquals(listOf("1", "1b", "2", "x"), NovelChapters.sorted(list, ascending = true).map { it.id })
        assertEquals(listOf("TL A", "TL B"), NovelChapters.translators(list))
        assertEquals(listOf("2", "1b"), NovelChapters.filtered(list, "", "TL A", null).map { it.id })
        assertEquals(listOf("1b"), NovelChapters.filtered(list, "", null, "es").map { it.id })
        assertEquals(listOf("2"), NovelChapters.filtered(list, "2", null, null).map { it.id })
        assertEquals("2", NovelChapters.neighbour(list, "1b", forward = true)?.id)
        assertNull(NovelChapters.neighbour(list, "1", forward = false))
        assertEquals(setOf("1b", "2", "x"), NovelChapters.dedupe(list, "TL A").map { it.id }.toSet())
        assertEquals("e/s/1", NovelRepositoryImpl.downloadId(list[1]))
        assertEquals("Ch. 2 · Chapter 2.0", NovelRepositoryImpl.chapterLabel(list[0]))
    }

    @Test
    fun paragraphQueueSkipsBlanksAndEnds() {
        val q = ParagraphQueue()
        q.load(listOf("", "a", " ", "b"), 0)
        assertEquals(1 to "a", q.current())
        assertEquals(3 to "b", q.peekNext())
        q.advance()
        assertEquals(3 to "b", q.current())
        assertNull(q.peekNext())
        q.advance()
        assertTrue(q.isEmpty)
        q.load(listOf("a", "b"), 9)
        assertTrue(q.isEmpty)
    }

    @Test
    fun settingsSanitized() {
        val r = NovelReaderSettings(font = "comic", sizeSp = 99, lineHeight = Float.NaN, paragraphSpacingDp = -1, background = "red").sanitized()
        assertEquals("serif", r.font)
        assertEquals(40, r.sizeSp)
        assertEquals(1.6f, r.lineHeight, 0f)
        assertEquals(0, r.paragraphSpacingDp)
        assertEquals("theme", r.background)
        val t = TtsSettings(speed = 9f, endpoint = " http://x/v1 ", voice = "  ").sanitized()
        assertEquals(2f, t.speed, 0f)
        assertEquals("http://x/v1", t.endpoint)
        assertNull(t.voice)
    }

    @Test
    fun ttsRequests() {
        val base = TtsSettings()
        try {
            CloudTts.request(TtsEngine.OPENAI, "hi", base, null, "https://o/v1", "https://e/v1")
            fail("missing key")
        } catch (e: TtsException) {
            assertEquals(TtsFailure.MISSING_KEY, e.reason)
        }
        val openai = CloudTts.request(TtsEngine.OPENAI, "hi", base.copy(voice = "nova"), "sk-1", "https://o/v1", "https://e/v1")
        assertEquals("https://o/v1/audio/speech", openai.url)
        assertEquals("Bearer sk-1", openai.headers["Authorization"])
        assertTrue(openai.body.contains("\"voice\":\"nova\"") && openai.body.contains("\"model\":\"gpt-4o-mini-tts\""))
        try {
            CloudTts.request(TtsEngine.OPENAI, "hi", base.copy(voice = "marin", model = "tts-1"), "sk", "https://o/v1", "https://e/v1")
            fail("marin is not a tts-1 voice")
        } catch (e: TtsException) {
            assertEquals(TtsFailure.MISSING_VOICE, e.reason)
        }
        val eleven = CloudTts.request(TtsEngine.ELEVENLABS, "hi", base.copy(voice = "abc 1"), "xi", "https://o/v1", "https://e/v1")
        assertEquals("https://e/v1/text-to-speech/abc%201?output_format=mp3_44100_128", eleven.url)
        assertEquals("xi", eleven.headers["xi-api-key"])
        assertTrue(eleven.body.contains("eleven_multilingual_v2"))
        val local = CloudTts.request(TtsEngine.OPENAI_COMPATIBLE, "hi", base.copy(endpoint = "http://127.0.0.1:8880/v1/audio/speech"), null, "", "")
        assertEquals("http://127.0.0.1:8880/v1/audio/speech", local.url)
        assertNull(local.headers["Authorization"])
        assertEquals("http://127.0.0.1:8880/v1", CloudTts.compatibleBase("http://127.0.0.1:8880/v1/"))
        assertNull(CloudTts.compatibleBase("not a url"))
        assertNull(CloudTts.compatibleBase("http://u:p@host/v1"))
    }

    @Test
    fun cacheKeyIgnoresSpeedButNotVoice() {
        val a = CloudTts.cacheKey(TtsEngine.OPENAI, TtsSettings(voice = "nova", speed = 1f), "text")
        assertEquals(a, CloudTts.cacheKey(TtsEngine.OPENAI, TtsSettings(voice = "nova", speed = 1.5f), "text"))
        assertNotEquals(a, CloudTts.cacheKey(TtsEngine.OPENAI, TtsSettings(voice = "alloy"), "text"))
        assertEquals(64, a.length)
    }

    @Test
    fun voiceParsers() {
        val eleven = CloudTts.parseElevenLabsVoices("""{"voices":[{"voice_id":"v1","name":"Rachel","labels":{"accent":"american"}}]}""")
        assertEquals("v1", eleven.single().id)
        assertEquals("Rachel", eleven.single().label)
        assertEquals(listOf("af_heart", "bf_emma"), CloudTts.parseCompatibleVoices("""{"voices":["af_heart","bf_emma"]}""").map { it.id })
        assertEquals("x", CloudTts.parseCompatibleVoices("""[{"id":"x","name":"X"}]""").single().id)
        assertTrue(CloudTts.parseCompatibleVoices("garbage").isEmpty())
        assertTrue("marin" !in CloudTts.openAiVoices("tts-1"))
    }

    private fun server(handler: (RecordedRequest) -> MockResponse): MockWebServer = MockWebServer().apply {
        dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = handler(request)
        }
        start(InetAddress.getByName("127.0.0.1"), 0)
    }

    @Test
    fun synthesizeWritesAudioAndCaches() = runTest {
        var calls = 0
        val s = server { req ->
            calls++
            assertEquals("POST", req.method)
            assertEquals("/v1/audio/speech", req.url.encodedPath)
            assertEquals("Bearer key", req.headers["Authorization"])
            assertTrue(req.body!!.utf8().contains("\"input\":\"Hello\""))
            MockResponse.Builder().code(200).addHeader("Content-Type", "audio/mpeg").body("ID3fakeaudio").build()
        }
        s.use {
            val base = "http://127.0.0.1:${s.port}/v1"
            val tts = CloudTts(OkHttpClient(), openAiBase = base, elevenLabsBase = base)
            val target = tmp.newFolder("tts").resolve("a.mp3")
            tts.synthesize(TtsEngine.OPENAI, "Hello", TtsSettings(voice = "nova"), "key", target)
            assertEquals("ID3fakeaudio", target.readText())
            tts.synthesize(TtsEngine.OPENAI, "Hello", TtsSettings(voice = "nova"), "key", target)
            assertEquals(1, calls)
        }
    }

    @Test
    fun synthesizeRejectsJsonAndHttpErrors() = runTest {
        val s = server { req ->
            if (req.url.encodedPath.endsWith("/speech")) {
                MockResponse.Builder().code(200).addHeader("Content-Type", "application/json").body("{}").build()
            } else {
                MockResponse.Builder().code(401).body("no").build()
            }
        }
        s.use {
            val base = "http://127.0.0.1:${s.port}/v1"
            val tts = CloudTts(OkHttpClient(), openAiBase = base, elevenLabsBase = base)
            try {
                tts.synthesize(TtsEngine.OPENAI, "x", TtsSettings(voice = "nova"), "k", tmp.root.resolve("b.mp3"))
                fail("json is not audio")
            } catch (e: TtsException) {
                assertEquals(TtsFailure.NOT_AUDIO, e.reason)
            }
            try {
                tts.synthesize(TtsEngine.ELEVENLABS, "x", TtsSettings(voice = "v"), "k", tmp.root.resolve("c.mp3"))
                fail("401")
            } catch (e: TtsException) {
                assertEquals(TtsFailure.HTTP, e.reason)
                assertEquals(401, e.httpCode)
            }
            assertFalse(tmp.root.resolve("b.mp3").exists())
        }
    }
}
