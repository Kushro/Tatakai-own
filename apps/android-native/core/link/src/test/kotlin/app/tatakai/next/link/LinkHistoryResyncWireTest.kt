// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.coroutines.runBlocking
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 2026-11 history per device: the `history_resync` event, the mode on the wire and both directions of compatibility. */
class LinkHistoryResyncWireTest {
    private val sid = "6f1d3c1e-8d0f-4b6a-9a76-0c2a8d9d2f11"

    private fun <T> withClient(block: suspend (LinkClient, FakeLink) -> T): T = FakeLink().use { fake ->
        runBlocking { block(LinkClient(fake.baseUrl.toHttpUrl(), OkHttpClient()), fake) }
    }

    @Test fun resyncEventsAreReadAndUnknownOnesSkipped() {
        val events = LinkWireMapping.events(
            """{"events":[
                {"type":"history_resync","mode":"push_all"},
                {"type":"history_resync","mode":"pull_replace","extra":1},
                {"type":"history_resync","mode":"something_new"},
                {"type":"future","payload":{"a":[1,2]}},
                {"type":"handoff","sessionId":"$sid","episode":2},
                {"mode":"push_all"}
            ]}""",
        )
        // the element without `type` cannot decode and is skipped without losing the others
        assertEquals(5, events.size)
        assertEquals(
            listOf(HistoryResyncMode.PUSH_ALL, HistoryResyncMode.PULL_REPLACE),
            events.mapNotNull(LinkWireMapping::historyResync),
        )
        assertNull(LinkWireMapping.handoff(events[0]), "a resync is not a hand-off")
        assertEquals(sid, LinkWireMapping.handoff(events[4])?.sessionId)
        assertTrue(LinkWireMapping.events("""{"events":"nope"}""").isEmpty())
        assertTrue(LinkWireMapping.events("""[]""").isEmpty())
    }

    @Test fun theLongPollDeliversResyncsNextToHandoffs() = withClient { c, fake ->
        fake.handler = { Triple(200, """{"events":[{"type":"history_resync","mode":"pull_replace"},{"type":"handoff","sessionId":"$sid"}]}""", true) }
        val events = assertIs<LinkResult.Ok<List<PhoneEventDto>>>(c.events(5)).value
        assertEquals(HistoryResyncMode.PULL_REPLACE, LinkWireMapping.historyResync(events[0]))
        assertEquals(sid, LinkWireMapping.handoff(events[1])?.sessionId)
    }

    @Test fun modesAndPushResultsAreOptionalOnTheWire() {
        // an older PC: no mode, no ignored
        val old = LinkHttp.json.decodeFromString(LinkHistoryPushResultDto.serializer(), """{"merged":2}""")
        assertEquals(LinkHistoryPushResultDto(2, ignored = false, mode = null), old)
        val ignored = LinkHttp.json.decodeFromString(LinkHistoryPushResultDto.serializer(), """{"merged":0,"ignored":true,"mode":"receive_only"}""")
        assertTrue(ignored.ignored)
        assertEquals(LinkHistoryMode.RECEIVE_ONLY, LinkHistoryMode.fromWire(ignored.mode))
        val page = LinkHttp.json.decodeFromString(LinkHistoryPageDto.serializer(), """{"items":[],"next_since_unix":5,"truncated":false,"mode":"paused"}""")
        assertEquals(LinkHistoryMode.PAUSED, LinkHistoryMode.fromWire(page.mode))
        assertNull(LinkHistoryMode.fromWire(null))
        assertNull(LinkHistoryMode.fromWire("later"))
        assertEquals(listOf("full", "receive_only", "paused"), LinkHistoryMode.entries.map { it.wire })
    }

    @Test fun modeAndResyncDoneRoutes() = withClient { c, fake ->
        fake.handler = { Triple(200, """{"mode":"receive_only","resync":false}""", true) }
        val m = assertIs<LinkResult.Ok<LinkHistoryModeDto>>(c.historyMode()).value
        assertEquals("receive_only", m.mode)
        assertFalse(m.resync)
        assertEquals("/v1/history/mode", fake.seen.last().path)
        fake.handler = { Triple(200, """{"notified":2}""", true) }
        assertEquals(2, assertIs<LinkResult.Ok<LinkResyncDoneDto>>(c.historyResyncDone()).value.notified)
        assertEquals("/v1/history/resync-done", fake.seen.last().path)
        assertEquals("POST", fake.seen.last().method)
    }
}
