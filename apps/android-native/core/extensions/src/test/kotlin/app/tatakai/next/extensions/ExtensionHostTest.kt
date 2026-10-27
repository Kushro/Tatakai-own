// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Container
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.time.Instant
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

/** Runs the dispatcher against a scripted engine that replays what the `synthetic-full` bundle produces. */
class ExtensionHostTest {
    private class FakeSession(val handlers: Map<String, (List<JsonElement>) -> JsInvocation>) : JsSession {
        var closed = false
        var calls = 0
        override suspend fun hasMethod(name: String) = name in handlers
        override suspend fun invoke(method: String, args: List<JsonElement>, collectChunks: Boolean): JsInvocation {
            calls++
            return (handlers[method] ?: throw ExtError.NotImplemented("no $method"))(args)
        }
        override fun close() { closed = true }
    }

    private class FakeEngine(val make: () -> FakeSession) : JsEngine {
        val sessions = mutableListOf<FakeSession>()
        override suspend fun open(manifest: ExtensionManifest, bundle: String, host: JsHost): JsSession = make().also { sessions += it }
    }

    private object NoHost : JsHost {
        override val networkPolicy = NetworkPolicy.NONE
        override suspend fun fetch(request: HostFetchRequest): HostFetchResponse = throw ExtError.PermissionDenied()
        override fun log(level: String, message: String) = Unit
    }

    private fun chunk(provider: String, priority: Int, vararg results: JsonObject) = buildJsonObject {
        put("provider", provider)
        put("kind", "stream")
        put("results", JsonArray(results.map { it.withPriority(priority) }))
        put("diagnostic", buildJsonObject {
            put("provider", provider); put("status", "ok"); put("resultCount", results.size); put("durationMs", 0); put("attempts", 1)
        })
    }

    private fun JsonObject.withPriority(p: Int) = JsonObject(this + ("providerPriority" to JsonPrimitive(p)))

    private fun result(name: String, url: String, quality: String, type: String, lang: String, subs: Int = 0) = buildJsonObject {
        put("source", name); put("url", url); put("quality", quality); put("sourceType", type); put("audioLanguage", lang)
        put("headers", buildJsonObject { put("Referer", "https://$name.example.invalid/") })
        put("subtitles", JsonArray(List(subs) { buildJsonObject { put("url", "https://cdn/s.vtt"); put("label", "English"); put("language", "en") } }))
    }

    private val manifestText = Fixtures.text("synthetic-full/manifest.json")
    private val manifest = ManifestParser.parse(manifestText)

    private fun syntheticSession() = FakeSession(
        mapOf(
            "streamAll" to { _ ->
                JsInvocation(
                    JsonNull,
                    listOf(
                        // Deliberately emitted out of order: the host must sort by providerPriority, then quality desc.
                        chunk(
                            "beta", 1,
                            result("beta", "https://cdn.beta.example.invalid/mp4/1/3_480.mp4?t=a28b1e3e", "480p", "mp4", "en"),
                            result("beta", "https://cdn.beta.example.invalid/mp4/1/3_720.mp4?t=a28b1e3e", "720p", "mp4", "en"),
                        ),
                        chunk("alpha", 0, result("alpha", "https://cdn.alpha.example.invalid/hls/1/3/master.m3u8?t=a28b1e3e", "1080p", "hls", "ja", subs = 1)),
                    ),
                )
            },
            "listProviders" to { _ ->
                JsInvocation(JsonArray(listOf("alpha", "beta").map { buildJsonObject { put("name", it); put("kind", "stream") } }))
            },
            "runProvider" to { args ->
                val name = args[0].jsonPrimitive.content
                if (name == "gamma") throw ExtError.BundleError("Unknown provider: gamma")
                JsInvocation(chunk(name, 0, result(name, "https://cdn.$name.example.invalid/a.m3u8", "720p", "hls", "ja")))
            },
            "customHome" to { args ->
                JsInvocation(
                    buildJsonObject {
                        put("sections", JsonArray(listOf(buildJsonObject {
                            put("title", "Featured (${args[0].jsonPrimitive.content})")
                            put("items", JsonArray(listOf(buildJsonObject { put("id", "x1"); put("title", "Synthetic One") })))
                        })))
                    },
                )
            },
        ),
    )

    private fun host(engine: FakeEngine, enabled: Boolean = true, compat: CompatLevel = CompatLevel.FULL, now: () -> Instant = { Instant.parse("2026-01-01T00:00:00Z") }) =
        ExtensionHost(engine, { NoHost }, now).also { it.register(LoadedExtension(manifest, "// bundle", compat, enabled)) }

    @Test
    fun streamMatchesTheFixtureExpectations() = runTest {
        val expected = Json.parseToJsonElement(Fixtures.text("synthetic-full/expected.stream.json")).jsonObject
        val h = host(FakeEngine { syntheticSession() })
        val body = h.call("synthetic-full", "stream", buildJsonObject {
            put("anilistId", 1); put("episode", 3); put("titles", JsonArray(listOf(JsonPrimitive("Synthetic")))); put("stream", "0")
        })
        val sources = body["sources"]!!.jsonArray
        val want = expected["sources"]!!.jsonArray
        assertEquals(expected["count"]!!.jsonPrimitive.int, body["count"]!!.jsonPrimitive.int)
        assertEquals(want.size, sources.size)
        want.forEachIndexed { i, w ->
            val wo = w.jsonObject
            val g = sources[i].jsonObject
            for (key in listOf("providerName", "type", "quality", "url", "audioLanguage")) {
                assertEquals(wo[key]!!.jsonPrimitive.content, g[key]!!.jsonPrimitive.content, "sources[$i].$key")
            }
            assertEquals(wo["providerPriority"]!!.jsonPrimitive.int, g["providerPriority"]!!.jsonPrimitive.int, "sources[$i].providerPriority")
            assertEquals(wo["subtitleCount"]!!.jsonPrimitive.int, g["subtitles"]!!.jsonArray.size, "sources[$i].subtitleCount")
        }
        val statuses = body["providerStatus"]!!.jsonArray.map { it.jsonObject["provider"]!!.jsonPrimitive.content }.toSet()
        assertEquals(setOf("alpha", "beta"), statuses)
    }

    @Test
    fun streamMapsToModelStreamSources() = runTest {
        val h = host(FakeEngine { syntheticSession() })
        val body = h.call("synthetic-full", "stream", buildJsonObject { put("anilistId", 1) })
        val streams = SourceNormalizer.toStreamSources(body, "synthetic-full")
        assertEquals(listOf(Container.HLS, Container.MP4, Container.MP4), streams.map { it.container })
        assertEquals("alpha", streams[0].provider)
        assertEquals("synthetic-full", streams[0].extensionId)
        assertEquals("vtt", streams[0].subtitles.single().format)
        assertEquals("https://alpha.example.invalid/", streams[0].headers["Referer"])
    }

    @Test
    fun embedsAndTorrentsAreNotPlayable() {
        val body = buildJsonObject {
            put("sources", JsonArray(listOf(
                SourceNormalizer.normalize(buildJsonObject { put("url", "https://x.test/embed/1"); put("sourceType", "custom") }, "p", 0),
                SourceNormalizer.normalize(buildJsonObject { put("url", "magnet:?xt=urn:btih:1") }, "p", 0),
                SourceNormalizer.normalize(buildJsonObject { put("url", "https://x.test/v.mp4") }, "p", 0),
            )))
        }
        assertEquals(1, SourceNormalizer.toStreamSources(body, "e").size)
    }

    @Test
    fun healthProvidersAndCustomRoutes() = runTest {
        val h = host(FakeEngine { syntheticSession() })
        val health = h.call("synthetic-full", "health")
        assertEquals("synthetic", health["namespace"]!!.jsonPrimitive.content)
        val providers = h.call("synthetic-full", "providers")
        assertEquals(2, providers["count"]!!.jsonPrimitive.int)
        val one = h.call("synthetic-full", "providers/alpha")
        assertEquals("ok", one["status"]!!.jsonPrimitive.content)
        assertFailsWith<ExtError.NotFound> { h.call("synthetic-full", "providers/gamma") }
        val home = h.call("synthetic-full", "custom/home", buildJsonObject { put("sourceId", "demo") })
        assertEquals(1, home["sections"]!!.jsonArray.size)
        assertEquals("x1", home["sections"]!!.jsonArray[0].jsonObject["items"]!!.jsonArray[0].jsonObject["id"]!!.jsonPrimitive.content)
    }

    @Test
    fun errorStatusesFollowTheContract() = runTest {
        val h = host(FakeEngine { syntheticSession() })
        assertEquals(400, assertFailsWith<ExtError.BadRequest> { h.call("synthetic-full", "custom/home") }.status)
        assertEquals(404, assertFailsWith<ExtError.NotFound> { h.call("nope", "stream") }.status)
        assertEquals(404, assertFailsWith<ExtError.NotFound> { h.call("synthetic-full", "bogus/route") }.status)
        // "torrent" is not listed in manifest.apiServer.routes of the fixture
        assertEquals(404, assertFailsWith<ExtError.NotFound> { h.call("synthetic-full", "torrent") }.status)
        // the fixture bundle has no manga methods
        assertEquals(501, assertFailsWith<ExtError.NotImplemented> { h.call("synthetic-full", "manga/chapters") }.status)
        assertEquals(400, assertFailsWith<ExtError.BadRequest> { h.call("synthetic-full", "manga/pages") }.status)
    }

    @Test
    fun unsupportedAndDisabledExtensionsRefuseCalls() = runTest {
        val engine = FakeEngine { syntheticSession() }
        assertFailsWith<ExtError.Unsupported> { host(engine, compat = CompatLevel.UNSUPPORTED).call("synthetic-full", "health") }
        assertFailsWith<ExtError.Unsupported> { host(engine, enabled = false).call("synthetic-full", "stream") }
        assertTrue(engine.sessions.isEmpty(), "no runtime may be created for an unsupported / disabled extension")
    }

    @Test
    fun resultsAreCachedUntilRefresh() = runTest {
        val engine = FakeEngine { syntheticSession() }
        var now = Instant.parse("2026-01-01T00:00:00Z")
        val h = host(engine, now = { now })
        val params = buildJsonObject { put("anilistId", 1) }
        h.call("synthetic-full", "stream", params)
        val second = h.call("synthetic-full", "stream", params)
        assertEquals(true, second["cached"]!!.jsonPrimitive.content.toBoolean())
        assertEquals(1, engine.sessions.single().calls)
        h.call("synthetic-full", "stream", buildJsonObject { put("anilistId", 1); put("refresh", "1") })
        assertEquals(2, engine.sessions.single().calls)
        now = now.plusSeconds(601)
        h.call("synthetic-full", "stream", params)
        assertEquals(3, engine.sessions.single().calls)
    }

    @Test
    fun timeoutDiscardsTheRuntimeAndRecreatesItOnNextCall() = runTest {
        var first = true
        val engine = FakeEngine {
            if (first) {
                first = false
                FakeSession(mapOf("streamAll" to { _ -> throw ExtError.Timeout() }))
            } else {
                syntheticSession()
            }
        }
        val h = host(engine)
        assertFailsWith<ExtError.Timeout> { h.call("synthetic-full", "stream", buildJsonObject { put("anilistId", 1) }) }
        assertTrue(engine.sessions[0].closed)
        val ok = h.call("synthetic-full", "stream", buildJsonObject { put("anilistId", 1) })
        assertEquals(3, ok["count"]!!.jsonPrimitive.int)
        assertEquals(2, engine.sessions.size)
    }

    @Test
    fun sourceOptionsFollowTheLegacyQueryKeys() {
        val h = ExtensionHost(FakeEngine { syntheticSession() }, { NoHost })
        val o = h.sourceOptions(buildJsonObject {
            put("anilistId", "21"); put("titles[]", JsonArray(listOf(JsonPrimitive("A"), JsonPrimitive("B"))))
            put("preferredLanguages", "ja, en"); put("timeoutMs", 5000)
        })
        assertEquals(21, o["anilistId"]!!.jsonPrimitive.int)
        assertEquals(1, o["episode"]!!.jsonPrimitive.int)
        assertEquals("1080p", o["resolution"]!!.jsonPrimitive.content)
        assertEquals(listOf("A", "B"), o["titles"]!!.jsonArray.map { it.jsonPrimitive.content })
        assertEquals(listOf("ja", "en"), o["preferredLanguages"]!!.jsonArray.map { it.jsonPrimitive.content })
        assertEquals(5000, o["providerOptions"]!!.jsonObject["timeoutMs"]!!.jsonPrimitive.int)
    }
}
