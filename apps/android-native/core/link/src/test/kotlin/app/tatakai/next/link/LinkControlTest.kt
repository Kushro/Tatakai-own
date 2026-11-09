// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** parity wave B (16 §5 r.7 g, worker r): `GET /v1/player`, `POST /v1/control`, `GET /v1/events` and the mDNS discovery rules. */
class LinkControlTest {
    private val sid = "6f1d3c1e-8d0f-4b6a-9a76-0c2a8d9d2f11"

    private fun <T> withClient(block: suspend (LinkClient, FakeLink) -> T): T = FakeLink().use { fake ->
        runBlocking { block(LinkClient(fake.baseUrl.toHttpUrl(), OkHttpClient()), fake) }
    }

    @Test fun playerStateDecodesTheCamelCaseWire() = withClient { c, fake ->
        fake.handler = { Triple(200, """{"playing":true,"title":"Frieren","episode":3,"positionSecs":90,"mediaId":"154587"}""", true) }
        val dto = assertNotNull(assertIs<LinkResult.Ok<PlayerStateDto?>>(c.playerState()).value)
        val st = LinkWireMapping.playerState(dto)
        assertTrue(st.playing)
        assertEquals(90, st.positionSec)
        assertEquals("154587", st.animeId)
        assertNull(st.durationSec)
        assertEquals("/v1/player", fake.seen.single().path)
    }

    @Test fun playerStateIsNullOn404() = withClient { c, fake ->
        fake.handler = { Triple(404, """{"error":{"code":"no_player","message":"x"}}""", true) }
        assertNull(assertIs<LinkResult.Ok<PlayerStateDto?>>(c.playerState()).value)
    }

    @Test fun controlSendsTheCanonicalCommands() = withClient { c, fake ->
        fake.handler = { Triple(204, "", true) }
        assertIs<LinkResult.Ok<Unit>>(c.playerAction(app.tatakai.next.model.RemoteAction.PLAY_PAUSE.id))
        assertEquals("""{"command":"toggle"}""", fake.seen.last().body)
        assertEquals("POST", fake.seen.last().method)
        assertEquals("/v1/control", fake.seen.last().path)
        assertIs<LinkResult.Ok<Unit>>(c.handoff("21", 3, 120))
        val body = Json.parseToJsonElement(fake.seen.last().body).jsonObject
        assertEquals("open", body["command"]!!.jsonPrimitive.content)
        assertEquals("21", body["mediaId"]!!.jsonPrimitive.content)
        assertEquals(3, body["episode"]!!.jsonPrimitive.int)
        assertEquals(120, body["positionSecs"]!!.jsonPrimitive.int)
    }

    @Test fun controlWithoutPlayerFailsWith409() = withClient { c, fake ->
        fake.handler = { Triple(409, """{"error":{"code":"no_player","message":"x"}}""", true) }
        val f = assertIs<LinkResult.Failure>(c.playerAction("next"))
        assertEquals(409, f.error.status())
    }

    @Test fun eventsLongPoll() = withClient { c, fake ->
        fake.handler = { Triple(204, "", true) }
        assertEquals(emptyList(), assertIs<LinkResult.Ok<List<PhoneEventDto>>>(c.events()).value)
        fake.handler = {
            Triple(200, """{"events":[{"type":"handoff","sessionId":"$sid","mediaId":"21","episode":3,"positionSecs":75},{"type":"future"}]}""", true)
        }
        val events = assertIs<LinkResult.Ok<List<PhoneEventDto>>>(c.events(99)).value
        assertEquals(2, events.size)
        val h = assertNotNull(LinkWireMapping.handoff(events[0]))
        assertEquals(sid, h.sessionId)
        assertEquals(75, h.positionSec)
        assertNull(LinkWireMapping.handoff(events[1]))
        assertNull(LinkWireMapping.handoff(PhoneEventDto(type = "handoff", sessionId = "../x")))
        assertEquals("/v1/events", fake.seen.last().path)
    }

    @Test fun discoveryParsesTheCanonicalTxt() {
        val fp = "ab".repeat(32)
        val pc = assertNotNull(DiscoveryWire.parse("Desk", "192.168.1.20", 47321, mapOf("v" to "1", "name" to "My PC", "id" to sid, "fp" to fp.uppercase())))
        assertEquals("My PC", pc.pc.name)
        assertEquals(sid, pc.pc.instanceId)
        assertEquals(fp, pc.fingerprint)
        assertNull(DiscoveryWire.parse("x", "192.168.1.20", 1, mapOf("v" to "2", "fp" to fp)))
        assertNull(DiscoveryWire.parse("x", "fe80::1", 1, mapOf("v" to "1", "fp" to fp)))
        assertNull(DiscoveryWire.parse("x", "192.168.1.20", 1, mapOf("v" to "1", "fp" to "short")))
        assertEquals("ABAB ABAB", DiscoveryWire.displayFingerprint(fp).take(9))
    }

    @Test fun discoveryPayloadIsAValidPairingPayload() {
        val fp = "0f".repeat(32)
        val pc = assertNotNull(DiscoveryWire.parse("Desk", "10.0.0.5", 47321, mapOf("v" to "1", "name" to "Living room PC", "fp" to fp)))
        assertNull(DiscoveryWire.payload(pc, "short", 1_000))
        val text = assertNotNull(DiscoveryWire.payload(pc, "abcd-efgh-jk", 1_000))
        val parsed = assertIs<PayloadResult.Valid>(PairingPayload.parse(text, 1_000)).payload
        assertEquals("10.0.0.5", parsed.host)
        assertEquals(47321, parsed.port)
        assertEquals(fp, parsed.caSha256)
        assertEquals("ABCDEFGHJK", parsed.code)
        assertEquals("Living room PC", parsed.pcName)
    }
}
