// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertTrue

/** parity wave A (worker q, history.link-sync.client): `/v1/history` on the wire (15 §3.4, shared with desktop §3.8). */
class LinkHistoryRouteTest {

    private fun <T> withClient(block: suspend (LinkClient, FakeLink) -> T): T = FakeLink().use { fake ->
        runBlocking { block(LinkClient(fake.baseUrl.toHttpUrl(), OkHttpClient()), fake) }
    }

    private val row = LinkHistoryRow(
        animeId = "154587",
        title = "Sousou no Frieren",
        episodeId = "154587:1",
        episode = 1,
        progressSec = 180,
        durationSec = 1440,
        watchedAt = "2026-11-09T12:00:00.000Z",
        anilistId = 154587,
    )

    @Test
    fun sinceDecodesThePageAndSendsTheCursor() = withClient { c, fake ->
        fake.handler = {
            Triple(
                200,
                """{"items":[{"animeId":"154587","title":"Sousou no Frieren","episodeId":"154587:1","episode":1,"progressSec":180,
                   "durationSec":1440,"completed":false,"watchedAt":"2026-11-09T12:00:00.000Z","anilistId":154587}],
                   "next_since_unix":1762689600,"truncated":true}""",
                true,
            )
        }
        val page = assertIs<LinkResult.Ok<LinkHistoryPageDto>>(c.historySince(1762600000, limit = 9999)).value
        assertEquals(1, page.items.size)
        assertEquals(180, page.items.single().progressSec)
        assertEquals(1762689600, page.nextSinceUnix)
        assertTrue(page.truncated)
        assertEquals("GET /v1/history", fake.seen.single().let { "${it.method} ${it.path}" })
        assertEquals("v1", fake.seen.single().contractHeader)
    }

    @Test
    fun pushSendsCamelCaseRowsAndReadsMerged() = withClient { c, fake ->
        fake.handler = { Triple(200, """{"merged":1}""", true) }
        val result = assertIs<LinkResult.Ok<LinkHistoryPushResultDto>>(c.historyPush(listOf(row))).value
        assertEquals(1, result.merged)
        val seen = fake.seen.single()
        assertEquals("POST /v1/history", "${seen.method} ${seen.path}")
        val item = Json.parseToJsonElement(seen.body).jsonObject["items"]!!.jsonArray.single().jsonObject
        assertEquals("154587:1", item["episodeId"]!!.jsonPrimitive.content)
        assertEquals("180", item["progressSec"]!!.jsonPrimitive.content)
        assertEquals("2026-11-09T12:00:00.000Z", item["watchedAt"]!!.jsonPrimitive.content)
    }

    @Test
    fun pushRefusesEmptyOrOversizedBatchesWithoutTouchingTheWire() = withClient { c, fake ->
        assertIs<LinkResult.Failure>(c.historyPush(emptyList()))
        assertIs<LinkResult.Failure>(c.historyPush(List(LinkClient.MAX_HISTORY_ROWS + 1) { row }))
        assertTrue(fake.seen.isEmpty())
    }

    @Test
    fun clearIsADelete() = withClient { c, fake ->
        fake.handler = { Triple(204, "", true) }
        assertIs<LinkResult.Ok<Unit>>(c.historyClear())
        assertEquals("DELETE /v1/history", fake.seen.single().let { "${it.method} ${it.path}" })
    }
}
