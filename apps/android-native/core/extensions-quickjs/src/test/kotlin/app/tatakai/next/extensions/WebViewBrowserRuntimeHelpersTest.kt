// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertTrue

/** The pure helpers of [WebViewBrowserRuntime] (no WebView involved). */
class WebViewBrowserRuntimeHelpersTest {
    @Test
    fun evaluationEnvelopeIsDecodedTwice() {
        // evaluateJavascript hands back the JSON of the returned string
        val ok = JsonPrimitive("""{"ok":true,"v":{"url":"https://sololatino.net/","n":2}}""").toString()
        val v = WebViewBrowserRuntime.parseEvaluation(ok).jsonObject
        assertEquals("https://sololatino.net/", v["url"]!!.jsonPrimitive.content)
        assertEquals(JsonNull, WebViewBrowserRuntime.parseEvaluation(JsonPrimitive("""{"ok":true,"v":null}""").toString()))
        val err = assertFailsWith<BrowserError.Failed> {
            WebViewBrowserRuntime.parseEvaluation(JsonPrimitive("""{"ok":false,"e":"x is not defined"}""").toString())
        }
        assertEquals("x is not defined", err.message)
        assertFailsWith<BrowserError.Failed> { WebViewBrowserRuntime.parseEvaluation("null") }
        assertFailsWith<BrowserError.Failed> { WebViewBrowserRuntime.parseEvaluation(null) }
    }

    @Test
    fun expressionsAreWrapped() {
        val w = WebViewBrowserRuntime.wrapExpression(" document.title ; ")
        assertTrue(w.contains("\ndocument.title\n"))
        assertTrue(w.startsWith("(function(){try{"))
    }

    @Test
    fun cookieHeaderParsing() {
        assertEquals(
            listOf("cf_clearance" to "abc=def", "a" to ""),
            WebViewBrowserRuntime.parseCookieHeader("cf_clearance=abc=def; a=; =bad; noeq"),
        )
        assertTrue(WebViewBrowserRuntime.parseCookieHeader(null).isEmpty())
        val c = WebViewBrowserRuntime.cookieJson("cf_clearance", "v", "sololatino.net")
        assertEquals("sololatino.net", c["domain"]!!.jsonPrimitive.content)
        assertEquals("true", c["session"]!!.jsonPrimitive.content)
    }
}
