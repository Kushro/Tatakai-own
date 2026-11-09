// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.Quality
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.StremioAddon
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertNull
import kotlin.test.assertTrue

// parity wave B (16 §5 t.6c, worker t): Stremio manifests, stream mapping, Cinemeta episode ids and the install / stream round trip.
class StremioWireTest {
    private val manifest = """
        {"id":"org.example.anime","name":"Anime Streams","version":"1.4.0","description":"Test addon","types":["anime","series"],
         "resources":["catalog",{"name":"stream","types":["anime"],"idPrefixes":["kitsu"]}],"catalogs":[]}
    """.trimIndent()

    private fun addon(id: String = "org.example.anime") = StremioAddon(
        id = id, name = "Anime Streams", version = "1.4.0", manifestUrl = "https://a.example/manifest.json", types = listOf("anime"), installedAtMs = 1,
    )

    @Test
    fun parsesManifestResources() {
        val m = StremioWire.parseManifest(manifest)
        assertEquals("org.example.anime", m.id)
        assertTrue(m.hasStreams)
        assertEquals(listOf("kitsu"), m.idPrefixes)
        assertEquals(listOf("anime"), m.streamTypes)
        assertFailsWith<IllegalArgumentException> { StremioWire.parseManifest("""{"id":"x","name":"x","version":"1"}""") }
        assertFailsWith<IllegalArgumentException> { StremioWire.parseManifest("[]") }
        assertTrue(!StremioWire.parseManifest("""{"id":"x","name":"x","version":"1","resources":["catalog"]}""").hasStreams)
    }

    @Test
    fun manifestUrls() {
        assertEquals("https://host.example/abc/manifest.json", StremioWire.normalizeManifestUrl("stremio://host.example/abc/manifest.json"))
        assertNull(StremioWire.normalizeManifestUrl("ftp://host/manifest.json"))
        assertEquals("https://host.example/abc", StremioWire.baseUrl("https://host.example/abc/manifest.json"))
        assertEquals(
            "https://host.example/abc/stream/series/tt123:1:2.json",
            StremioWire.resourceUrl("https://host.example/abc/manifest.json", "stream", "series", "tt123:1:2"),
        )
    }

    @Test
    fun streamsBecomeWebAndTorrentCandidates() {
        val body = Json.parseToJsonElement(
            """{"streams":[
                {"name":"Server\n1080p","title":"1080p Sub ES","url":"https://cdn.example/a.m3u8",
                 "behaviorHints":{"proxyHeaders":{"request":{"Referer":"https://site.example/"}}}},
                {"name":"Server","url":"https://cdn.example/b.mp4"},
                {"name":"Dash","url":"https://cdn.example/c.mpd"},
                {"name":"Torrentio","title":"[SubsPlease] Show - 01 (1080p).mkv\n👤 42","infoHash":"ABCDEF0123456789ABCDEF0123456789ABCDEF01","fileIdx":3}
            ]}""",
        )
        val c = StremioWire.streamsToCandidates(addon(), body)
        assertEquals(3, c.size)
        assertEquals("stremio:org.example.anime", c[0].extId)
        assertEquals("Anime Streams", c[0].extName)
        assertEquals(SourceKind.WEB, c[0].kind)
        assertEquals("HLS", c[0].container)
        assertEquals(Quality.P1080, c[0].quality)
        assertEquals(mapOf("Referer" to "https://site.example/"), c[0].web?.headers)
        assertEquals("Server #2", c[1].label)
        assertEquals("MP4", c[1].container)
        assertEquals(SourceKind.TORRENT, c[2].kind)
        assertEquals("abcdef0123456789abcdef0123456789abcdef01", c[2].torrent?.infoHash)
        assertEquals(42, c[2].torrent?.seeders)
        assertEquals("SubsPlease", c[2].label)
    }

    @Test
    fun cinemetaEpisodeIds() {
        val meta = Json.parseToJsonElement(
            """{"id":"tt1","type":"series","videos":[
                {"id":"tt1:0:1","season":0,"episode":1},
                {"id":"tt1:1:1","season":1,"episode":1},{"id":"tt1:1:2","season":1,"episode":2},
                {"id":"tt1:2:1","season":2,"episode":1}]}""",
        ) as JsonObject
        assertEquals("tt1:1:2", StremioWire.episodeVideoId(meta, 2))
        assertEquals("tt1:2:1", StremioWire.episodeVideoId(meta, 3))
        assertNull(StremioWire.episodeVideoId(meta, 9))
        val metas = Json.parseToJsonElement("""[{"id":"tt1","name":"Sousou no Frieren"},{"id":"tt2","name":"Other"}]""") as kotlinx.serialization.json.JsonArray
        assertEquals("tt1", (StremioWire.pickMeta(metas, listOf("Sousou no Frieren!"))?.get("id") as kotlinx.serialization.json.JsonPrimitive).content)
    }

    @Test
    fun installThenResolveStreamsByKitsuId() = runBlocking {
        val server = MockWebServer().apply {
            dispatcher = object : Dispatcher() {
                override fun dispatch(request: RecordedRequest): MockResponse {
                    val path = request.url.encodedPath
                    return when {
                        path.endsWith("/manifest.json") -> MockResponse.Builder().code(200).addHeader("Content-Type", "application/json").body(manifest).build()
                        path == "/addon/stream/anime/kitsu:46474:3.json" -> MockResponse.Builder().code(200).addHeader("Content-Type", "application/json")
                            .body("""{"streams":[{"name":"Alpha","url":"https://cdn.example/ep3.m3u8"}]}""").build()
                        else -> MockResponse.Builder().code(404).build()
                    }
                }
            }
            start()
        }
        try {
            val dir = Files.createTempDirectory("stremio").toFile()
            val client = OkHttpClient()
            val repo = StremioAddonRepositoryImpl(client, dir, clock = { 7L })
            val installed = repo.install(server.url("/addon/manifest.json").toString()).getOrThrow()
            assertEquals("Anime Streams", installed.name)
            assertEquals(7L, installed.installedAtMs)
            assertEquals(listOf("org.example.anime"), repo.addons.first().map { it.id })
            val source = StremioCandidateSource(repo, client)
            val media = Media(
                ref = MediaRef(anilistId = 154587, kitsuId = 46474), kind = MediaKind.ANIME, title = "Frieren", titles = emptyMap(),
                cover = null, banner = null, color = null, synopsis = null, genres = emptyList(), episodes = 28, status = null,
                season = null, year = 2023, score = null, sourceProvider = "anilist",
            )
            val candidates = source.candidates(media, 3)
            assertEquals(listOf("https://cdn.example/ep3.m3u8"), candidates.map { it.web?.url })
            repo.setEnabled("org.example.anime", false)
            assertTrue(source.candidates(media, 3).isEmpty())
            dir.deleteRecursively()
        } finally {
            server.close()
        }
    }
}
