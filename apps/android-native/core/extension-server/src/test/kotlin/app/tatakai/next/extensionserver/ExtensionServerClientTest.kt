// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.extensionserver.FakeServer.Companion.fail
import app.tatakai.next.extensionserver.FakeServer.Companion.health
import app.tatakai.next.extensionserver.FakeServer.Companion.ndjson
import app.tatakai.next.extensionserver.FakeServer.Companion.ok
import kotlinx.coroutines.flow.toList
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import okhttp3.OkHttpClient
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ExtensionServerClientTest {
    private val client = ExtensionServerClient(OkHttpClient.Builder().followRedirects(false).build())

    private fun conn(origin: String) = ExtensionServerConnection("c1", "srv-1", origin, "Desk", token = "tok-123")

    @Test
    fun healthUnwrapsTheEnvelopeAndKeepsTheNewFields() = FakeServer { ok(health()) }.use { s ->
        val h = runBlocking { client.health(s.origin) }
        assertEquals(1, h.protocolVersion)
        assertEquals("srv-1", h.serverId)
        assertTrue(h.pairingRequired)
        assertEquals("/server/", h.dashboardPath)
        val req = s.take()
        assertEquals("/api/server/v1/health", req.url.encodedPath)
        assertEquals(null, req.headers["Authorization"])
    }

    @Test
    fun pairPostsTheCodeAndLabel() = FakeServer { ok("""{"token":"t-1","role":"viewer","deviceId":"d-1"}""") }.use { s ->
        val token = runBlocking { client.pair(s.origin, "abc-123", "Tatakai Next (Pixel)") }
        assertEquals("t-1", token.token)
        assertFalse("t-1" in token.toString())
        val req = s.take()
        assertEquals("POST", req.method)
        assertEquals("/api/server/v1/auth/pair", req.url.encodedPath)
        val body = Json.parseToJsonElement(req.body!!.utf8()).jsonObject
        assertEquals("abc-123", body["code"]!!.jsonPrimitive.content)
        assertEquals("Tatakai Next (Pixel)", body["label"]!!.jsonPrimitive.content)
    }

    @Test
    fun loginIsAViewerLogin() = FakeServer { ok("""{"token":"t-2","role":"viewer","deviceId":"d-2"}""") }.use { s ->
        runBlocking { client.login(s.origin, "pw", "phone") }
        val body = Json.parseToJsonElement(s.take().body!!.utf8()).jsonObject
        assertEquals("viewer", body["role"]!!.jsonPrimitive.content)
        assertEquals("pw", body["password"]!!.jsonPrimitive.content)
    }

    @Test
    fun errorsKeepOnlyTheServerCode() = FakeServer { fail(401, "pairing_expired") }.use { s ->
        val e = assertFailsWith<ExtensionServerException> { runBlocking { client.pair(s.origin, "x", "y") } }
        assertEquals("pairing_expired", e.code)
        assertEquals(401, e.status)
        assertFalse(e.unauthorized, "an expired code is not a revoked token")
        assertFalse("secret" in e.message.orEmpty())
    }

    @Test
    fun revokedTokenIsUnauthorized() = FakeServer { fail(401, "authentication_required") }.use { s ->
        val e = assertFailsWith<ExtensionServerException> { runBlocking { client.sources(conn(s.origin)) } }
        assertTrue(e.unauthorized)
        assertEquals("Bearer tok-123", s.take().headers["Authorization"])
    }

    @Test
    fun sourcesDecodeAndIgnoreUnknownFields() = FakeServer {
        ok("""[{"id":"srv-1/a","extensionId":"a","name":"Anime A","ecosystem":"aniyomi","kind":"watch","enabled":true,
              "available":true,"reason":"","languages":["es"],"future":1},
             {"id":"srv-1/a~read","name":"Anime A · Manga","ecosystem":"aniyomi","kind":"read"}]""")
    }.use { s ->
        val list = runBlocking { client.sources(conn(s.origin)) }
        assertEquals(listOf("watch", "read"), list.map { it.kind })
        assertEquals("aniyomi", list[0].ecosystem)
    }

    @Test
    fun resolveStreamsEveryNdjsonSnapshot() {
        val lines = listOf(
            """{"sources":[],"subtitles":[],"statuses":[{"provider":"srv-1/a","name":"A","ecosystem":"aniyomi","status":"loading","count":0}]}""",
            """{"sources":[{"url":"http://x/1","sourceType":"hls","providerKey":"srv-1/a"}],"subtitles":[],"statuses":[{"provider":"srv-1/a","status":"loading","count":1}]}""",
            "",
            """{"sources":[{"url":"http://x/1","sourceType":"hls","providerKey":"srv-1/a"},{"url":"http://x/2","sourceType":"mp4","providerKey":"srv-1/a"}],"subtitles":[],"statuses":[{"provider":"srv-1/a","status":"ok","count":2}]}""",
        )
        FakeServer { ndjson(lines) }.use { s ->
            val snapshots = runBlocking { client.resolve(conn(s.origin), buildJsonObject { put("episode", 3) }).toList() }
            assertEquals(listOf(0, 1, 2), snapshots.map { it.sources.size })
            assertFalse(snapshots[1].complete)
            assertTrue(snapshots.last().complete)
            val req = s.take()
            assertEquals("/api/server/v1/resolve", req.url.encodedPath)
            assertEquals(ExtensionServerClient.NDJSON, req.headers["Accept"])
            assertEquals("Bearer tok-123", req.headers["Authorization"])
            assertEquals(3, Json.parseToJsonElement(req.body!!.utf8()).jsonObject["episode"]!!.jsonPrimitive.content.toInt())
        }
    }

    @Test
    fun resolveAcceptsTheAggregatedJsonAnswer() = FakeServer {
        ok("""{"sources":[{"url":"http://x/1","sourceType":"hls"}],"subtitles":[],"statuses":[{"provider":"p","status":"ok","count":1}]}""")
    }.use { s ->
        val snapshots = runBlocking { client.resolve(conn(s.origin), JsonObject(emptyMap())).toList() }
        assertEquals(1, snapshots.single().sources.size)
    }

    @Test
    fun resolveFailsOnAMalformedLineAndOnHttpErrors() {
        FakeServer { ndjson(listOf("""{"sources":[]}""", "not json")) }.use { s ->
            val e = assertFailsWith<ExtensionServerException> { runBlocking { client.resolve(conn(s.origin), JsonObject(emptyMap())).toList() } }
            assertEquals(ExtensionServerException.BAD_RESPONSE, e.code)
        }
        FakeServer { fail(401, "device_revoked") }.use { s ->
            val e = assertFailsWith<ExtensionServerException> { runBlocking { client.resolve(conn(s.origin), JsonObject(emptyMap())).toList() } }
            assertTrue(e.unauthorized)
        }
    }

    @Test
    fun parseSnapshotLineIsLenientAboutExtraFields() {
        val snap = client.parseSnapshotLine("""{"sources":[{"url":"u"},3],"statuses":[{"provider":"p","status":"partial","error":"x","extra":true}],"more":1}""")
        assertEquals(1, snap.sourceObjects.size)
        assertEquals("partial", snap.statuses.single().status)
    }

    @Test
    fun publicHttpIsRejectedBeforeAnyRequest() {
        val e = assertFailsWith<ExtensionServerException> { runBlocking { client.health("http://example.org:8787") } }
        assertEquals(ExtensionServerException.INSECURE_HOST, e.code)
        val r = assertFailsWith<ExtensionServerException> {
            runBlocking { client.resolve(conn("http://8.8.8.8"), JsonObject(emptyMap())).toList() }
        }
        assertEquals(ExtensionServerException.INSECURE_HOST, r.code)
    }

    @Test
    fun unreachableServerIsReportedAsUnreachable() {
        val port = java.net.ServerSocket(0, 1, java.net.InetAddress.getByName("127.0.0.1")).use { it.localPort }
        val e = assertFailsWith<ExtensionServerException> { runBlocking { client.health("http://127.0.0.1:$port") } }
        assertEquals(ExtensionServerException.UNREACHABLE, e.code)
    }
}
