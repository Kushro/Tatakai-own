// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtLogLevel
import kotlinx.coroutines.awaitCancellation
import kotlinx.coroutines.test.runTest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

/** The session-side checks of `__host_browser` (desktop runtime.rs `browser_op`) against a scripted runtime. */
class BrowserBridgeTest {
    private class FakeRuntime : BrowserRuntime {
        var seq = 0
        val closed = mutableListOf<String>()
        val navigated = mutableListOf<String>()
        var evaluateResult: JsonElement = JsonPrimitive("ok")
        var hang = false
        override suspend fun newBrowser(extId: String, allowedDomains: List<String>, options: JsonObject): BrowserId = "b${++seq}"
        override suspend fun navigate(id: BrowserId, url: String) {
            if (hang) awaitCancellation()
            navigated += url
        }
        override suspend fun evaluate(id: BrowserId, expression: String): JsonElement = evaluateResult
        override suspend fun cookies(id: BrowserId, urls: List<String>): List<JsonObject> =
            urls.map { buildJsonObject { put("name", "cf_clearance"); put("value", "v"); put("domain", "sololatino.net") } }
        override suspend fun close(id: BrowserId) { closed += id }
    }

    private val domains = listOf("sololatino.net")

    private fun env(json: String) = Json.parseToJsonElement(json).jsonObject

    private fun ok(json: String) = env(json).also { assertTrue(it["ok"]!!.jsonPrimitive.boolean, json) }["value"]

    private fun error(json: String): String = env(json).also { assertFalse(it["ok"]!!.jsonPrimitive.boolean, json) }["error"]!!.jsonPrimitive.content

    @Test
    fun lifecycleAndLimits() = runTest {
        val rt = FakeRuntime()
        val events = mutableListOf<Pair<ExtLogLevel, String>>()
        val bridge = BrowserBridge(rt, "latino", domains) { l, m -> events += l to m }
        val a = ok(bridge.call("new", "", """{"headless":false,"timeout":5}"""))!!.jsonPrimitive.content
        assertTrue(bridge.openedBrowser)
        assertTrue(events.any { it.first == ExtLogLevel.WARN })
        ok(bridge.call("new", "", "{}"))
        assertEquals("ChromeDP browser limit reached", error(bridge.call("new", "", "{}")))

        ok(bridge.call("navigate", a, "https://www.sololatino.net/x"))
        assertEquals(listOf("https://www.sololatino.net/x"), rt.navigated)
        assertEquals("ChromeDP URL is outside the provider website", error(bridge.call("navigate", a, "https://evil.example/")))
        assertEquals("ok", ok(bridge.call("evaluate", a, "1+1"))!!.jsonPrimitive.content)
        assertEquals("Invalid ChromeDP expression", error(bridge.call("evaluate", a, "x".repeat(BrowserPolicy.MAX_EXPRESSION_BYTES + 1))))

        val cookies = ok(bridge.call("cookies", a, """["https://sololatino.net/"]"""))!!.jsonObject["cookies"]!!.jsonArray
        assertEquals("cf_clearance", cookies.single().jsonObject["name"]!!.jsonPrimitive.content)
        assertEquals("ChromeDP cookie URLs are required", error(bridge.call("cookies", a, "[]")))
        assertEquals("ChromeDP URL is outside the provider website", error(bridge.call("cookies", a, """["https://evil.example/"]""")))

        ok(bridge.call("close", a, ""))
        assertEquals(listOf(a), rt.closed)
        assertEquals("ChromeDP browser is closed", error(bridge.call("navigate", a, "https://sololatino.net/")))
        assertEquals("ChromeDP browser is closed", error(bridge.call("evaluate", "b999", "1")))
        assertEquals("Unsupported ChromeDP browser method: print", error(bridge.call("print", "b2", "")))

        bridge.closeAll()
        assertEquals(listOf(a, "b2"), rt.closed)
        assertEquals(0, bridge.openCount)
    }

    @Test
    fun resultsAreCappedAndOperationsTimeOut() = runTest {
        val rt = FakeRuntime()
        val bridge = BrowserBridge(rt, "latino", domains)
        val id = ok(bridge.call("new", "", """{"timeout":1}"""))!!.jsonPrimitive.content
        rt.evaluateResult = JsonPrimitive("x".repeat(BrowserPolicy.MAX_RESULT_BYTES))
        assertEquals("ChromeDP response is too large", error(bridge.call("evaluate", id, "big")))
        rt.hang = true
        assertEquals("ChromeDP operation timed out", error(bridge.call("navigate", id, "https://sololatino.net/")))
    }

    @Test
    fun noDomainsMeansNoBrowser() = runTest {
        val bridge = BrowserBridge(FakeRuntime(), "x", emptyList())
        assertEquals("ChromeDP requires a permitted public provider website", error(bridge.call("new", "", "{}")))
    }
}
