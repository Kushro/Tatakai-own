// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.model.Container
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.sourceKey
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertTrue

class ServerCandidatesTest {
    private val origin = "http://192.168.1.20:8787"
    private val conn = ExtensionServerConnection("c1", "srv-1", origin, "Desk", "tok")
    private val client = ExtensionServerClient(okhttp3.OkHttpClient())

    private fun snapshot(json: String) = client.parseSnapshotLine(json)

    @Test
    fun groupsByProviderAndKeepsOnlyProxiedSameOriginMedia() {
        val snap = snapshot(
            """{"sources":[
              {"url":"$origin/api/server/v1/media/t1","sourceType":"hls","quality":"1080p","sourceName":"Vidstream","providerKey":"srv-1/ani-1",
               "providerName":"AnimeFLV · Desk","subtitles":[{"url":"$origin/api/server/v1/media/s1","lang":"es","label":"Español"},
               {"url":"https://evil.example/sub.vtt","lang":"en"}],"audioLanguage":"ja"},
              {"url":"$origin/api/server/v1/media/t2","isM3U8":true,"quality":"720p","sourceName":"Vidstream","providerKey":"srv-1/ani-1","providerName":"AnimeFLV · Desk"},
              {"url":"https://cdn.example/direct.m3u8","sourceType":"hls","providerKey":"srv-1/ani-1"},
              {"url":"http://192.168.1.99:8787/api/server/v1/media/t3","sourceType":"hls","providerKey":"srv-1/ani-1"},
              {"url":"https://embed.example/e/1","sourceType":"embed","providerKey":"srv-1/ani-1"},
              {"url":"$origin/api/server/v1/media/t4","sourceType":"mp4","sourceName":"Main","providerKey":"srv-1/local-cs","extensionName":"CloudStream X"},
              {"sourceType":"torrent","infoHash":"0123456789abcdef0123456789abcdef01234567","title":"[SubsPlease] Show - 03 (1080p).mkv","providerKey":"srv-1/toko"}
            ],"subtitles":[],"statuses":[]}""",
        )
        val list = ServerCandidates.map(conn, snap)
        assertEquals(4, list.size, list.toString())

        val first = list[0]
        assertEquals("server:srv-1/ani-1", first.extId)
        assertEquals("AnimeFLV · Desk", first.extName)
        assertEquals(SourceKind.WEB, first.kind)
        assertEquals("Vidstream", first.label)
        assertEquals(Quality.P1080, first.quality)
        assertEquals(listOf("es"), first.subs)
        assertEquals(1, first.web!!.subtitles.size, "subtitles off the server origin are dropped")
        assertTrue(first.web!!.headers.isEmpty())
        assertEquals(Container.HLS, first.toStream()!!.container)

        // Same label inside one provider gets the quality appended (unique sourceKey).
        assertEquals("Vidstream 720p", list[1].label)
        assertEquals(Container.HLS, list[1].toStream()!!.container)

        val cs = list[2]
        assertEquals("server:srv-1/local-cs", cs.extId)
        assertEquals("CloudStream X · Desk", cs.extName)
        assertEquals("Main", cs.label)

        val torrent = list[3]
        assertEquals(SourceKind.TORRENT, torrent.kind)
        assertEquals("0123456789abcdef0123456789abcdef01234567", torrent.torrent!!.infoHash)

        list.forEach { c ->
            assertFalse('|' in c.extId)
            assertEquals(3, sourceKey(c).split('|', limit = 3).size)
        }
    }

    @Test
    fun extIdNeverContainsAPipeAndDoesNotRepeatTheServerId() {
        assertEquals("server:srv-1/abc", ServerCandidates.extIdOf(conn, "srv-1/abc"))
        assertEquals("server:srv-1/other_x", ServerCandidates.extIdOf(conn, "other|x"))
        assertEquals("server:srv-1/default", ServerCandidates.extIdOf(conn, "srv-1/"))
        assertTrue(ServerCandidates.isServerExtId("server:srv-1/abc"))
    }

    @Test
    fun publicHttpMediaIsDroppedEvenOnTheSameHostName() {
        val publicConn = conn.copy(url = "https://pc.example.org")
        val snap = snapshot(
            """{"sources":[{"url":"http://pc.example.org/api/server/v1/media/t","sourceType":"hls","providerKey":"srv-1/a"},
               {"url":"https://pc.example.org/api/server/v1/media/u","sourceType":"hls","providerKey":"srv-1/a"}]}""",
        )
        val list = ServerCandidates.map(publicConn, snap)
        assertEquals(listOf("https://pc.example.org/api/server/v1/media/u"), list.map { it.web!!.url })
    }
}
