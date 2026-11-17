// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import app.tatakai.next.model.LinkSubtitleTrack
import kotlinx.coroutines.runBlocking
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertIs
import kotlin.test.assertNotNull
import kotlin.test.assertNull

/** 0.4.0: the source's subtitles of a Link session (status `subtitles`, `subtitles/{n}.ass|.vtt`, the loopback allow-list). */
class LinkSubtitlesTest {
    private val sid = "6f1d3c1e-8d0f-4b6a-9a76-0c2a8d9d2f11"
    private val token = "0123456789abcdef0123456789abcdef"

    private fun decode(json: String) = LinkRepositoryImpl.statusOf(LinkHttp.json.decodeFromString(SessionStateDto.serializer(), json))

    @Test
    fun theStatusListsTheSourceTracksAndAnOlderPcNone() {
        val s = decode(
            """{"state":"ready","generation":1,"instance_id":"i","profile":"p-none","subtitles_state":"idle","subtitles":[
            {"index":0,"language":"eng","title":"English","codec":"ass","default":true,"forced":false,"text":true,"formats":["ass","vtt"]},
            {"index":1,"language":"spa","title":"Spanish(Latin America)","codec":"ass","default":false,"forced":false,"text":true,"formats":["ass","vtt"]},
            {"index":2,"codec":"hdmv_pgs_subtitle","default":false,"forced":false,"text":false,"formats":[]},
            {"index":3,"codec":"subrip","default":false,"forced":true,"text":true,"formats":["vtt","srt"],"future_field":1}]}""",
        )
        assertEquals("idle", s.subtitlesState)
        val t = assertNotNull(s.subtitles)
        assertEquals(LinkSubtitleTrack(0, "eng", "English", "ass", default = true, forced = false, text = true, formats = listOf("ass", "vtt")), t[0])
        assertEquals("Spanish(Latin America)", t[1].title)
        assertEquals(false, t[2].text)
        // unknown formats are dropped, so the phone never asks for a file the contract does not name
        assertEquals(listOf("vtt"), t[3].formats)
        assertEquals(true to false, t[3].hasVtt to t[3].hasAss)
        val old = decode("""{"state":"ready","generation":1,"instance_id":"i","profile":"p-none"}""")
        assertNull(old.subtitles)
        assertNull(old.subtitlesState)
    }

    @Test
    fun aSubtitleAnswerSaysWhetherToWait() {
        assertEquals(SubtitleReadiness.READY, LinkRepositoryImpl.subtitleReadinessOf(LinkResult.Ok("WEBVTT")))
        for (pending in listOf(LinkApiError.SegmentWaitTimeout, LinkApiError.SessionNotReady)) {
            assertEquals(SubtitleReadiness.PENDING, LinkRepositoryImpl.subtitleReadinessOf(LinkResult.Failure(LinkError.Api(pending))), pending.code)
        }
        assertEquals(SubtitleReadiness.PENDING, LinkRepositoryImpl.subtitleReadinessOf(LinkResult.Failure(LinkError.Transport.Timeout)))
        for (final in listOf(LinkApiError.SourceUnavailable, LinkApiError.SessionNotFound, LinkApiError.SessionGone)) {
            assertEquals(SubtitleReadiness.UNAVAILABLE, LinkRepositoryImpl.subtitleReadinessOf(LinkResult.Failure(LinkError.Api(final))), final.code)
        }
    }

    @Test
    fun theClientAsksForOneFileOfTheSession() = FakeLink().use { fake ->
        fake.handler = { Triple(200, "[Script Info]\nScriptType: v4.00+\n", true) }
        val c = LinkClient(fake.baseUrl.toHttpUrl(), OkHttpClient())
        val text = assertIs<LinkResult.Ok<String>>(runBlocking { c.subtitle(sid, 1, "ass") }).value
        assertEquals("[Script Info]\nScriptType: v4.00+\n", text)
        assertEquals("/v1/sessions/$sid/subtitles/1.ass", fake.seen.single().path)
        assertEquals("${fake.baseUrl.trimEnd('/')}/v1/sessions/$sid/subtitles/2.vtt", c.subtitleUrl(sid, 2, "vtt"))
        // nothing outside the contract leaves the phone
        for ((id, n, f) in listOf(Triple("x", 1, "vtt"), Triple(sid, -1, "vtt"), Triple(sid, 1, "srt"))) {
            assertIs<LinkResult.Failure>(runBlocking { c.subtitle(id, n, f) })
        }
        assertEquals(1, fake.seen.size)
    }

    @Test
    fun theLoopbackProxyServesSubtitleFilesOfTheSessionOnly() {
        for (file in listOf("subtitles/0.ass", "subtitles/12.vtt")) {
            val r = LinkLoopbackProxy.parseRequest("GET /$token/v1/sessions/$sid/$file HTTP/1.1\r\nHost: 127.0.0.1", token)
            assertEquals("/v1/sessions/$sid/$file", r?.path, file)
        }
        for (bad in listOf("subtitles/1.srt", "subtitles/a.vtt", "subtitles/../position", "subtitles/1.vtt/x", "x/1.vtt", "subtitles/1234.vtt")) {
            assertNull(LinkLoopbackProxy.parseRequest("GET /$token/v1/sessions/$sid/$bad HTTP/1.1", token), bad)
        }
        assertEquals(true, LinkLoopbackProxy.isSessionPath("/v1/sessions/$sid/subtitles/3.vtt"))
    }
}
