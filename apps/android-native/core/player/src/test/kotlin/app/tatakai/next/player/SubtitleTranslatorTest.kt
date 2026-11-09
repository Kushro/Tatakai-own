// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.TranslationProvider
import kotlinx.coroutines.test.runTest
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import okhttp3.OkHttpClient
import java.nio.file.Files
import kotlin.test.AfterTest
import kotlin.test.BeforeTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

// parity wave A (15 §5 p (g)): one MockWebServer round trip per provider, the cache and the pre-translation batches.
class SubtitleTranslatorTest {
    private lateinit var server: MockWebServer
    private val client = OkHttpClient()

    @BeforeTest
    fun setUp() {
        server = MockWebServer()
        server.start()
    }

    @AfterTest
    fun tearDown() = server.close()

    private fun respond(body: String, code: Int = 200) = server.enqueue(MockResponse.Builder().code(code).body(body).build())

    private fun config(provider: TranslationProvider, endpoint: String, key: String? = null, region: String = "", model: String = "") =
        TranslatorConfig(provider, "es", endpoint, model, region, key)

    @Test
    fun googleNeedsNoKey() = runTest {
        respond("""[[["Hola\n","Hello\n",null],["Adiós","Bye",null]],null,"en"]""")
        val out = SubtitleTranslators.create(config(TranslationProvider.GOOGLE, server.url("/translate_a/single").toString()), client)
            .translate(listOf("Hello", "Bye"))
        assertEquals(listOf("Hola", "Adiós"), out)
        val req = server.takeRequest()
        assertEquals("gtx", req.url.queryParameter("client"))
        assertEquals("es", req.url.queryParameter("tl"))
        assertEquals(null, req.headers["Authorization"])
    }

    @Test
    fun deeplSendsTheKeyHeader() = runTest {
        respond("""{"translations":[{"text":"Hola"},{"text":"Adiós"}]}""")
        val out = SubtitleTranslators.create(config(TranslationProvider.DEEPL, server.url("/v2/translate").toString(), key = "k:fx"), client)
            .translate(listOf("Hello", "Bye"))
        assertEquals(listOf("Hola", "Adiós"), out)
        val req = server.takeRequest()
        assertEquals("DeepL-Auth-Key k:fx", req.headers["Authorization"])
        assertTrue(req.body!!.utf8().contains("\"target_lang\":\"ES\""))
    }

    @Test
    fun deeplxUsesTheUrl() = runTest {
        respond("""{"code":200,"data":"Hola"}""")
        val out = SubtitleTranslators.create(config(TranslationProvider.DEEPLX, server.url("/translate").toString()), client).translate(listOf("Hello"))
        assertEquals(listOf("Hola"), out)
        assertFailsWith<TranslationException> { SubtitleTranslators.create(config(TranslationProvider.DEEPLX, ""), client) }
    }

    @Test
    fun azureNeedsKeyAndRegion() = runTest {
        respond("""[{"translations":[{"text":"Hola","to":"es"}]}]""")
        val out = SubtitleTranslators.create(config(TranslationProvider.AZURE, server.url("").toString(), key = "az", region = "westeurope"), client)
            .translate(listOf("Hello"))
        assertEquals(listOf("Hola"), out)
        val req = server.takeRequest()
        assertEquals("az", req.headers["Ocp-Apim-Subscription-Key"])
        assertEquals("westeurope", req.headers["Ocp-Apim-Subscription-Region"])
        assertEquals("3.0", req.url.queryParameter("api-version"))
        val e = assertFailsWith<TranslationException> { SubtitleTranslators.create(config(TranslationProvider.AZURE, "", key = "az"), client) }
        assertEquals(TranslationException.Kind.MISSING_REGION, e.kind)
    }

    @Test
    fun openAiCompatibleParsesNumberedLines() = runTest {
        respond("""{"choices":[{"message":{"content":"<think>x</think>1. Hola\n2. Adiós"},"finish_reason":"stop"}]}""")
        val out = SubtitleTranslators.create(config(TranslationProvider.OPENAI, server.url("/v1").toString(), key = "sk", model = "m"), client)
            .translate(listOf("Hello", "Bye"))
        assertEquals(listOf("Hola", "Adiós"), out)
        val req = server.takeRequest()
        assertEquals("/v1/chat/completions", req.url.encodedPath)
        assertEquals("Bearer sk", req.headers["Authorization"])
        assertEquals("http://h:1/v1/chat/completions", SubtitleTranslators.completeOpenAiUrl("http://h:1"))
    }

    @Test
    fun httpErrorsSurface() = runTest {
        respond("""{"message":"quota"}""", code = 456)
        val e = assertFailsWith<TranslationException> {
            SubtitleTranslators.create(config(TranslationProvider.DEEPL, server.url("/v2/translate").toString(), key = "k"), client).translate(listOf("x"))
        }
        assertEquals(TranslationException.Kind.HTTP, e.kind)
        assertTrue(e.message!!.contains("quota"))
    }

    @Test
    fun cacheAvoidsSecondCallAndPretranslateBatches() = runTest {
        val dir = Files.createTempDirectory("subs-translate").toFile()
        val cache = TranslationCache(dir)
        var calls = 0
        val translator = SubtitleTranslator { lines -> calls++; lines.map { "T:$it" } }
        val scope = TranslationCache.scope("ep1", config(TranslationProvider.GOOGLE, ""))
        val session = SubtitleTranslationSession(translator, cache, scope)
        assertEquals(listOf("T:a", "T:b"), session.translateLive(listOf("a", "b")))
        assertEquals(listOf("T:a"), session.translateLive(listOf("a")))
        assertEquals(1, calls)
        // A fresh cache instance reads the file: re-watching never calls the provider.
        val again = SubtitleTranslationSession(translator, TranslationCache(dir), scope)
        assertEquals(listOf("T:b"), again.translateLive(listOf("b")))
        assertEquals(1, calls)
        val progress = mutableListOf<Pair<Int, Int>>()
        val texts = (1..90).map { "line $it" }
        val failed = again.pretranslate(texts) { d, t -> progress += d to t }
        assertEquals(0, failed)
        assertEquals(90 to 90, progress.last())
        assertEquals(4, calls) // 3 batches of <= 40 lines
        assertTrue(cache.sizeBytes() > 0)
        cache.clear()
        assertEquals(0L, cache.sizeBytes())
        dir.deleteRecursively()
    }
}
