// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import java.net.ServerSocket
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

class LinkClientTest {
    private val sid = "6f1d3c1e-8d0f-4b6a-9a76-0c2a8d9d2f11"

    private fun <T> withClient(block: suspend (LinkClient, FakeLink) -> T): T = FakeLink().use { fake ->
        runBlocking { block(LinkClient(fake.baseUrl.toHttpUrl(), OkHttpClient()), fake) }
    }

    private fun errorBody(code: String) = """{"error":{"code":"$code","message":"ignored text"}}"""

    @Test
    fun capabilitiesDecodeAndSendTheContractHeader() = withClient { c, fake ->
        fake.handler = {
            Triple(
                200,
                """{"contract":"v1","server_version":"0.1","instance_id":"$sid","server_name":"PC","gpu":{"vendor":"amd","name":"x","vulkan":true},
            "encoders":["h264_amf"],"default_profile":"p-none","client_fg":"allowed","limits":{"max_ram_bytes":536870912,"segment_wait_secs":20},
            "profiles":[{"id":"p-none","label":"None","upscale":"none","output":{"width":1920,"height":1080},"measured_sustain":"unknown"},
            {"id":"p-anime4k-1080-fg2x","label":"A4K","upscale":"anime4k","fg":"frc_amf","output":{"width":1920,"height":1080},"measured_sustain":true}]}""",
                true,
            )
        }
        val caps = assertIs<LinkResult.Ok<CapabilitiesDto>>(c.capabilities()).value
        assertEquals("PC", caps.serverName)
        assertEquals(2, caps.profiles.size)
        assertEquals("frc_amf", caps.profiles[1].fg)
        assertEquals("v1", fake.seen.single().contractHeader)
        assertEquals("/v1/capabilities", fake.seen.single().path)
    }

    @Test
    fun createUrlSessionBuildsTheOpenApiBody() = withClient { c, fake ->
        fake.handler = {
            Triple(
                201,
                """{"session_id":"$sid","generation":1,"playback_path":"/v1/sessions/$sid/master.m3u8","segment_secs":2,"heartbeat_interval_secs":4,"idle_timeout_secs":120}""",
                true,
            )
        }
        val r = assertIs<LinkResult.Ok<SessionCreatedDto>>(
            c.createSession(SessionSource.Url("https://cdn.example/m.m3u8", mapOf("Referer" to "https://x/")), profile = "p-none", startSecs = 12.5),
        )
        assertEquals(sid, r.value.sessionId)
        assertEquals(fake.baseUrl + "v1/sessions/$sid/master.m3u8", c.playbackUrl(r.value.playbackPath))
        val body = Json.parseToJsonElement(fake.seen.single().body).jsonObject
        val src = body["source"]!!.jsonObject
        assertEquals("url", src["kind"]!!.jsonPrimitive.content)
        assertEquals("https://x/", src["headers"]!!.jsonObject["Referer"]!!.jsonPrimitive.content)
        assertEquals("p-none", body["profile"]!!.jsonPrimitive.content)
        assertEquals(12.5, body["start_secs"]!!.jsonPrimitive.content.toDouble())
        assertTrue("container" !in src, "unset optional fields must be omitted")
    }

    @Test
    fun heartbeatAndStopUseTheSessionRoutes() = withClient { c, fake ->
        fake.handler = { Triple(204, "", true) }
        assertIs<LinkResult.Ok<Unit>>(c.heartbeat(sid, 61.2, paused = false))
        assertIs<LinkResult.Ok<Unit>>(c.deleteSession(sid))
        assertEquals("POST /v1/sessions/$sid/position", fake.seen[0].let { "${it.method} ${it.path}" })
        assertEquals("DELETE /v1/sessions/$sid", fake.seen[1].let { "${it.method} ${it.path}" })
        val hb = Json.parseToJsonElement(fake.seen[0].body).jsonObject
        assertEquals("false", hb["paused"]!!.jsonPrimitive.content)
        assertEquals(61.2, hb["position_secs"]!!.jsonPrimitive.content.toDouble())
    }

    @Test
    fun sessionIdsThatAreNotUuidsNeverReachTheWire() = withClient { c, fake ->
        val r = c.deleteSession("../devices/self")
        assertEquals(LinkError.Api(LinkApiError.RequestInvalid), assertIs<LinkResult.Failure>(r).error)
        assertTrue(fake.seen.isEmpty())
    }

    @Test
    fun errorCodesMapOnlyWhenStatusMatchesTheTable() = withClient { c, fake ->
        suspend fun err(status: Int, code: String, header: Boolean = true): LinkError {
            fake.handler = { Triple(status, errorBody(code), header) }
            return assertIs<LinkResult.Failure>(c.capabilities()).error
        }
        assertEquals(LinkError.Api(LinkApiError.DeviceNotAuthorized), err(403, "device_not_authorized"))
        assertEquals(LinkError.Api(LinkApiError.SessionGone), err(410, "session_gone"))
        assertEquals(LinkError.Api(LinkApiError.UrlSourceDenied), err(403, "url_source_denied"))
        // right code, wrong status: not a contract error
        assertEquals(LinkError.Unexpected(LinkError.Unexpected.Reason.STATUS, 500), err(500, "session_gone"))
        // unknown code
        assertEquals(LinkError.Unexpected(LinkError.Unexpected.Reason.STATUS, 400), err(400, "made_up"))
        // missing contract header
        assertEquals(LinkError.Unexpected(LinkError.Unexpected.Reason.CONTRACT_HEADER, 403), err(403, "device_not_authorized", header = false))
    }

    @Test
    fun malformedSuccessBodyIsUnexpectedBody() = withClient { c, fake ->
        fake.handler = { Triple(200, """{"not":"capabilities"}""", true) }
        assertEquals(LinkError.Unexpected(LinkError.Unexpected.Reason.BODY, 200), assertIs<LinkResult.Failure>(c.capabilities()).error)
    }

    @Test
    fun refusedConnectionIsTransportRefused() = runBlocking {
        val port = ServerSocket(0).use { it.localPort } // closed again: nothing listens
        val c = LinkClient("http://127.0.0.1:$port/".toHttpUrl(), OkHttpClient())
        assertEquals(LinkError.Transport.Refused, assertIs<LinkResult.Failure>(c.capabilities()).error)
    }

    @Test
    fun appExceptionMapping() {
        assertEquals(app.tatakai.next.model.AppError.Offline, LinkError.Transport.Refused.toAppException().error)
        assertEquals(app.tatakai.next.model.AppError.Http(403), LinkError.Api(LinkApiError.DeviceNotAuthorized).toAppException().error)
        assertTrue(LinkError.Transport.Tls.Revoked.toAppException().cause is LinkException)
    }
}
