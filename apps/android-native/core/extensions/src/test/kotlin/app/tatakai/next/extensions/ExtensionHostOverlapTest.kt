// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.async
import kotlinx.coroutines.test.runCurrent
import kotlinx.coroutines.test.runTest
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

/**
 * BUG-128: on the phone the Toko `stream` route once ended in "InternalError: interrupted" and lost every web source. Two calls of
 * the same runtime overlapping (a call running while the extension is re-registered — switched off and on, updated — and the next
 * call queued behind it) must not interrupt each other, and a call that runs out of time keeps the sources it had found.
 */
class ExtensionHostOverlapTest {
    private val manifest = ManifestParser.parse(Fixtures.text("synthetic-full/manifest.json"))

    private object NoHost : JsHost {
        override val networkPolicy = NetworkPolicy.NONE
        override suspend fun fetch(request: HostFetchRequest): HostFetchResponse = throw ExtError.PermissionDenied()
        override fun log(level: String, message: String) = Unit
    }

    /** A runtime whose `streamAll` waits for [release] (or fails with [failWith]). */
    private class SlowSession(val name: String) : JsSession {
        val release = CompletableDeferred<Unit>()
        val started = CompletableDeferred<Unit>()
        var failWith: Exception? = null
        @Volatile var closed = false
        @Volatile var closedWhileRunning = false
        @Volatile private var running = false
        var calls = 0

        override suspend fun hasMethod(name: String) = name == "streamAll"

        override suspend fun invoke(method: String, args: List<JsonElement>, collectChunks: Boolean): JsInvocation {
            calls++
            running = true
            try {
                started.complete(Unit)
                release.await()
                failWith?.let { throw it }
                return JsInvocation(JsonNull, listOf(chunk(name)))
            } finally {
                running = false
            }
        }

        override fun close() {
            if (running) closedWhileRunning = true
            closed = true
        }
    }

    private class Engine(val make: (Int) -> SlowSession) : JsEngine {
        val sessions = mutableListOf<SlowSession>()
        override suspend fun open(manifest: ExtensionManifest, bundle: String, host: JsHost): JsSession =
            make(sessions.size).also { sessions += it }
    }

    private fun host(engine: Engine) = ExtensionHost(engine, { NoHost }).also { it.register(LoadedExtension(manifest, "// v1", CompatLevel.FULL, true)) }

    private fun params(episode: Int) = buildJsonObject { put("anilistId", 1); put("episode", episode) }

    private fun providers(body: JsonObject) = body["sources"]!!.jsonArray.map { it.jsonPrimitiveField("providerName") }

    private fun JsonElement.jsonPrimitiveField(key: String) = (this as JsonObject)[key]!!.jsonPrimitive.content

    @Test
    fun a_call_running_when_the_extension_is_registered_again_keeps_its_runtime_until_it_answers() = runTest {
        val engine = Engine { i -> SlowSession("run$i") }
        val h = host(engine)
        val first = async { h.call(manifest.id, "stream", params(1)) }
        runCurrent()
        val s1 = engine.sessions.single()
        s1.started.await()

        // The extension is switched off and on (or updated) while the call runs, and the next call queues behind it.
        h.register(LoadedExtension(manifest, "// v2", CompatLevel.FULL, true))
        val second = async { h.call(manifest.id, "stream", params(2)) }
        runCurrent()
        assertFalse(s1.closed, "the running call's runtime was closed under it (QuickJS: InternalError: interrupted)")

        s1.release.complete(Unit)
        assertEquals(listOf("run0"), providers(first.await()))
        runCurrent()
        assertTrue(s1.closed, "the replaced runtime is closed once its call answered")
        assertFalse(s1.closedWhileRunning)

        // The queued call runs on a fresh runtime of the new package.
        val s2 = engine.sessions[1]
        s2.release.complete(Unit)
        assertEquals(listOf("run1"), providers(second.await()))
        assertFalse(s2.closed)
        assertEquals(1, s1.calls)
    }

    @Test
    fun a_runtime_removed_while_a_call_runs_is_closed_after_that_call() = runTest {
        val engine = Engine { i -> SlowSession("run$i") }
        val h = host(engine)
        val call = async { h.call(manifest.id, "stream", params(1)) }
        runCurrent()
        val s1 = engine.sessions.single()
        h.unregister(manifest.id)
        assertFalse(s1.closed)
        s1.release.complete(Unit)
        assertEquals(listOf("run0"), providers(call.await()))
        assertTrue(s1.closed)
        assertFalse(s1.closedWhileRunning)
    }

    @Test
    fun a_stream_call_that_runs_out_of_time_lists_the_providers_that_answered_and_is_not_cached() = runTest {
        val engine = Engine { i ->
            SlowSession("run$i").apply {
                if (i == 0) failWith = ExtError.Timeout(JsInvocation(JsonNull, listOf(chunk("early"))))
                release.complete(Unit)
            }
        }
        val h = host(engine)
        val body = h.call(manifest.id, "stream", params(1))
        assertEquals(listOf("early"), providers(body))
        assertEquals("true", body["partial"]!!.jsonPrimitive.content)
        assertEquals(1, body["count"]!!.jsonPrimitive.int)
        assertTrue(engine.sessions[0].closed, "a timed-out runtime is discarded")
        // Not cached: the next look asks the extension again (fresh runtime).
        assertEquals(listOf("run1"), providers(h.call(manifest.id, "stream", params(1))))
    }

    @Test
    fun a_timeout_without_any_source_is_still_a_timeout() = runTest {
        val engine = Engine { i -> SlowSession("run$i").apply { failWith = ExtError.Timeout(); release.complete(Unit) } }
        val h = host(engine)
        val r = runCatching { h.call(manifest.id, "stream", params(1)) }
        assertTrue(r.exceptionOrNull() is ExtError.Timeout)
    }

    @Test
    fun a_caller_that_gives_up_mid_call_does_not_leave_its_half_run_call_in_the_next_one() = runTest {
        val engine = Engine { i -> SlowSession("run$i") }
        val h = host(engine)
        assertNull(withTimeoutOrNull(1_000) { h.call(manifest.id, "stream", params(1)) })
        val s1 = engine.sessions.single()
        assertTrue(s1.closed, "the runtime with the abandoned call is not reused")
        val next = async { h.call(manifest.id, "stream", params(2)) }
        runCurrent()
        engine.sessions[1].release.complete(Unit)
        assertEquals(listOf("run1"), providers(next.await()))
    }

    companion object {
        fun chunk(provider: String) = buildJsonObject {
            put("provider", provider)
            put("kind", "stream")
            put(
                "results",
                JsonArray(
                    listOf(
                        buildJsonObject {
                            put("source", provider); put("url", "https://cdn.$provider.example.invalid/master.m3u8"); put("quality", "1080p")
                            put("sourceType", "hls"); put("audioLanguage", "ja")
                        },
                    ),
                ),
            )
        }
    }
}
