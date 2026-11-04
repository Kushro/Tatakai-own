// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.EpisodeRange
import app.tatakai.next.model.INITIAL_RELIABILITY
import app.tatakai.next.model.Quality
import app.tatakai.next.model.ReleaseSource
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.VideoCodec
import app.tatakai.next.model.sourceKey
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.jsonObject
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** Same cases as the tests of apps/desktop-native/crates/tk-ext/src/candidates.rs (incl. the Toko-shaped torrent body). */
class CandidatesTest {
    private fun body(json: String): JsonObject = Json.parseToJsonElement(json).jsonObject

    @Test
    fun widenedWebEntriesAndMissingFields() {
        val c = Candidates.mapCandidates(
            "io.github.toko", "Toko",
            body(
                """{ "sources": [
                { "providerName": "toko", "server": "Alpha", "type": "hls", "url": "https://cdn.toko.example/a.m3u8", "quality": "1080p",
                  "subs": ["ES", "en"], "dubs": ["ja"], "note": "multi-sub", "headers": { "Referer": "https://toko.example/" }, "latencyMs": 180 },
                { "providerName": "beta", "type": "mp4", "url": "https://cdn.beta.example/b.mp4", "audioLanguage": "en",
                  "subtitles": [{ "url": "https://cdn.beta.example/es.vtt", "language": "es-419", "label": "Español" }] },
                { "type": "hls", "url": "https://x.example/x.m3u8" },
                { "providerName": "embed", "type": "embed", "url": "https://embed.example/e" },
                { "providerName": "empty", "type": "hls", "url": "" }
            ]}""",
            ),
        )
        assertEquals(3, c.size)
        assertEquals(Triple("Alpha", SourceKind.WEB, Quality.P1080), Triple(c[0].label, c[0].kind, c[0].quality))
        assertEquals(listOf("es", "en"), c[0].subs)
        assertEquals(listOf("ja"), c[0].dubs)
        assertEquals("multi-sub", c[0].note)
        assertEquals("HLS", c[0].container)
        assertEquals(180, c[0].web!!.latencyMs)
        assertEquals("https://toko.example/", c[0].web!!.headers["Referer"])
        assertEquals("io.github.toko|web|Alpha", sourceKey(c[0]))
        assertEquals("beta", c[1].label)
        assertEquals(Quality.UNKNOWN, c[1].quality)
        assertNull(c[1].note)
        assertEquals(listOf("es"), c[1].subs)
        assertEquals(listOf("en"), c[1].dubs)
        assertEquals(1, c[1].web!!.subtitles.size)
        assertEquals("io.github.toko", c[2].label)
        assertEquals("Toko", c[2].extName)
        assertTrue(c[2].subs.isEmpty() && c[2].dubs.isEmpty() && c[2].web!!.latencyMs == null)
        assertTrue(c.all { it.reliability == INITIAL_RELIABILITY && it.torrent == null })
    }

    @Test
    fun torrentEntriesCarryIndexData() {
        val c = Candidates.mapCandidates(
            "io.github.nyaa-own", "Nyaa",
            body(
                """{ "sources": [
                { "providerName": "nyaa", "group": "SubsPlease", "type": "torrent", "quality": "1080p", "codec": "H.264",
                  "url": "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=x", "seeders": 812, "leechers": 41, "fileSize": "1.38 GB",
                  "subs": ["en"], "dubs": ["ja"] },
                { "providerName": "nyaa", "sourceType": "torrent", "infoHash": "1111111111111111111111111111111111111111", "url": "https://nyaa.example/1.torrent",
                  "seeders": "6", "sizeBytes": 1900000000, "note": "sin confirmar" },
                { "providerName": "nyaa", "type": "torrent", "url": "https://nyaa.example/2.torrent" }
            ]}""",
            ),
        )
        assertEquals(2, c.size)
        val t = c[0].torrent!!
        assertEquals(SourceKind.TORRENT, c[0].kind)
        assertEquals("SubsPlease", c[0].label)
        assertEquals("H.264", c[0].container)
        assertEquals("abcdef0123456789abcdef0123456789abcdef01", t.infoHash)
        assertEquals(812, t.seeders)
        assertEquals(41, t.leechers)
        assertEquals(1_380_000_000L, t.sizeBytes)
        assertTrue(t.magnet!!.startsWith("magnet:"))
        assertNull(c[0].web)
        val t2 = c[1].torrent!!
        assertEquals("nyaa", c[1].label)
        assertEquals(6, t2.seeders)
        assertEquals(0, t2.leechers)
        assertEquals(1_900_000_000L, t2.sizeBytes)
        assertNull(t2.magnet)
        assertEquals("sin confirmar", c[1].note)
    }

    /** The shape Toko's torrent route returns (normalized `torrentAll` chunks of the nyaa / seadex / animetosho providers). */
    @Test
    fun tokoTorrentEntriesGetReleaseTags() {
        val c = Candidates.mapCandidates(
            "tatakai.extension.toko", "Toko",
            body(
                """{ "sources": [
                { "source": "nyaa", "providerName": "nyaa", "providerKey": "nyaa", "server": "", "type": "torrent", "sourceType": "torrent",
                  "isTorrent": true, "quality": "1080p", "audioLanguage": "ja", "headers": {}, "subtitles": [],
                  "url": "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=%5BSubsPlease%5D%20Show%20-%2005%20(1080p)",
                  "magnetLink": "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=%5BSubsPlease%5D%20Show%20-%2005%20(1080p)",
                  "torrentTitle": "[SubsPlease] Show - 05 (1080p) [A1B2C3D4].mkv", "releaseGroup": "SubsPlease",
                  "seeders": 812, "leechers": 41, "peers": 853, "fileSize": "1.4 GiB", "matchScore": 90, "fileFormat": "mkv" },
                { "source": "animetosho", "providerName": "animetosho", "server": "", "type": "torrent", "sourceType": "torrent", "quality": "auto",
                  "url": "https://animetosho.example/1.torrent",
                  "magnetLink": "magnet:?xt=urn:btih:VPG6AARWIEJXHW2N3YAEIQQZRSRGQEAB&dn=x",
                  "torrentTitle": "[Judas] Show S01 (BD 1080p HEVC x265 10bit Dual-Audio Multi-Subs) [Batch]", "seeders": 30 },
                { "source": "seadex", "providerName": "seadex", "server": "", "type": "torrent", "sourceType": "torrent", "quality": "1080p",
                  "url": "magnet:?xt=urn:btih:2222222222222222222222222222222222222222&dn=a",
                  "torrentTitle": "[Okay-Subs] Show - S01E01 (BD 1080p) [ABCD].mkv", "releaseGroup": "Okay-Subs", "seeders": 5 },
                { "source": "seadex", "providerName": "seadex", "server": "", "type": "torrent", "sourceType": "torrent", "isBest": false,
                  "url": "magnet:?xt=urn:btih:3333333333333333333333333333333333333333&dn=b", "torrentTitle": "[Alt] Show (WEB 720p)" },
                { "providerName": "nekobt", "type": "torrent", "seadexBest": true, "infoHash": "4444444444444444444444444444444444444444",
                  "url": "https://nekobt.example/t/4", "torrentTitle": "[Group] Show - 01 [WEB 1080p AV1]" }
            ]}""",
            ),
        )
        assertEquals(5, c.size)
        assertTrue(c.all { it.kind == SourceKind.TORRENT && it.web == null })
        val t = c[0].torrent!!
        assertEquals("SubsPlease", c[0].label)
        assertEquals(Quality.P1080, c[0].quality)
        assertEquals("abcdef0123456789abcdef0123456789abcdef01", t.infoHash)
        assertEquals(1_503_238_554L, t.sizeBytes)
        assertEquals(EpisodeRange(5, 5), c[0].tags.episodes)
        assertEquals("SubsPlease", c[0].tags.group)
        assertTrue(!c[0].tags.batch && !c[0].tags.seadexBest)
        assertEquals("Judas", c[1].label)
        assertEquals("abcde00236411373db4dde004442198ca2681001", c[1].torrent!!.infoHash)
        assertTrue(c[1].torrent!!.magnet!!.startsWith("magnet:"))
        assertEquals(Quality.P1080, c[1].quality)
        val g = c[1].tags
        assertTrue(g.batch && g.dualAudio && g.multiSub && g.bitDepth == 10 && g.season == 1)
        assertEquals(ReleaseSource.BD, g.source)
        assertEquals(VideoCodec.HEVC, g.codec)
        assertTrue(c[2].tags.seadexBest && !c[3].tags.seadexBest)
        assertEquals("Okay-Subs", c[2].label)
        assertEquals("Alt", c[3].label)
        assertEquals(Quality.P720, c[3].quality)
        assertTrue(c[4].tags.seadexBest)
        assertEquals(VideoCodec.AV1, c[4].tags.codec)
    }

    @Test
    fun hashesAndMagnetNames() {
        assertEquals("abcdef0123456789abcdef0123456789abcdef01", Candidates.hexInfoHash("ABCDEF0123456789ABCDEF0123456789ABCDEF01"))
        assertEquals("abcde00236411373db4dde004442198ca2681001", Candidates.hexInfoHash("VPG6AARWIEJXHW2N3YAEIQQZRSRGQEAB"))
        assertNull(Candidates.hexInfoHash("xyz"))
        assertNull(Candidates.hexInfoHash("1".repeat(32)))
        assertEquals("[G] Show - 01", Candidates.magnetName("magnet:?xt=urn:btih:aa&dn=%5BG%5D+Show+-+01&tr=x"))
        assertNull(Candidates.magnetName("magnet:?xt=urn:btih:aa"))
        assertEquals("50%", Candidates.magnetName("magnet:?dn=50%"))
        assertEquals("aa", Candidates.btih("magnet:?XT=URN:BTIH:aa&dn=x"))
    }

    @Test
    fun customExtensionsWithTorrentProvidersAskBothRoutes() {
        val base = """{"id":"x.toko","name":"Toko","version":"1.0.0","type":"custom","description":"d","permissions":[]}"""
        val m = ManifestParser.parse(base)
        assertEquals(listOf("stream"), Candidates.candidateRoutes(m, false))
        assertEquals(listOf("stream", "torrent"), Candidates.candidateRoutes(m, true))
        val toko = base.replace(""""permissions":[]""", """"permissions":[],"apiServer":{"routes":["sources","stream","torrent"]}""")
        assertEquals(listOf("stream", "torrent"), Candidates.candidateRoutes(ManifestParser.parse(toko), false))
        val noTorrent = base.replace(""""permissions":[]""", """"permissions":[],"apiServer":{"routes":["sources","stream"]}""")
        assertEquals(listOf("stream"), Candidates.candidateRoutes(ManifestParser.parse(noTorrent), true))
        val online = toko.replace(""""type":"custom"""", """"type":"onlinestream"""")
        assertEquals(listOf("stream", "torrent"), Candidates.candidateRoutes(ManifestParser.parse(online), false))
        val torrent = base.replace(""""type":"custom"""", """"type":"torrent"""")
        assertEquals(listOf("torrent"), Candidates.candidateRoutes(ManifestParser.parse(torrent), true))
        val gated = torrent.replace(""""permissions":[]""", """"permissions":[],"apiServer":{"routes":["sources"]}""")
        assertTrue(Candidates.candidateRoutes(ManifestParser.parse(gated), false).isEmpty())
        // apiServer without routes: the default list does not count as an explicit `torrent`
        val noRoutes = base.replace(""""permissions":[]""", """"permissions":[],"apiServer":{"namespace":"toko"}""")
        assertEquals(listOf("stream"), Candidates.candidateRoutes(ManifestParser.parse(noRoutes), false))
    }

    @Test
    fun exposesTorrentAllIsWordBounded() {
        assertTrue(Candidates.exposesTorrentAll("class X { async torrentAll(o, cb) {} }"))
        assertTrue(Candidates.exposesTorrentAll("torrentAll"))
        assertTrue(!Candidates.exposesTorrentAll("const mytorrentAll = 1; torrentAll2(); \$torrentAll"))
    }

    @Test
    fun repeatedLabelsGetUniqueKeys() {
        val labels = Candidates.mapCandidates(
            "e", "E",
            body(
                """{ "sources": [
                { "providerName": "beta", "type": "mp4", "url": "https://b.example/720.mp4", "quality": "720p" },
                { "providerName": "beta", "type": "mp4", "url": "https://b.example/480.mp4", "quality": "480p" },
                { "providerName": "beta", "type": "mp4", "url": "https://b.example/x.mp4" },
                { "providerName": "beta", "type": "mp4", "url": "https://b.example/y.mp4" }
            ]}""",
            ),
        ).map { it.label }
        assertEquals(listOf("beta", "beta 480p", "beta #2", "beta #3"), labels)
        assertTrue(Candidates.mapCandidates("e", "E", JsonObject(emptyMap())).isEmpty())
    }

    @Test
    fun sizes() {
        assertEquals(734_003_200L, Candidates.sizeOf(JsonPrimitive("700 MiB")))
        assertEquals(920_000_000L, Candidates.sizeOf(JsonPrimitive("0,92 GB")))
        assertNull(Candidates.sizeOf(JsonPrimitive("12 parsecs")))
        assertEquals(5L, Candidates.sizeOf(JsonPrimitive(5)))
    }
}
