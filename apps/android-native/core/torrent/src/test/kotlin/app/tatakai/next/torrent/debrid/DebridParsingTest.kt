// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.debrid

import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.TorrentInfo
import app.tatakai.next.model.torrent.DebridService
import app.tatakai.next.model.torrent.DebridStatus
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNotNull
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** parity wave A (worker q, sources.debrid.*): recorded answers of the four services. */
class DebridParsingTest {
    private val h1 = "a".repeat(40)
    private val h2 = "b".repeat(40)

    @Test
    fun accountAnswers() {
        val rd = parseUser(DebridService.REAL_DEBRID, """{"id":1,"username":"neo","premium":86400,"expiration":"2026-12-31T00:00:00.000Z","type":"premium"}""")
        assertEquals("neo", rd?.username)
        assertEquals(java.time.Instant.parse("2026-12-31T00:00:00Z").toEpochMilli(), rd?.premiumUntilMs)
        val tb = parseUser(DebridService.TORBOX, """{"success":true,"data":{"email":"neo@example.org","premium_expires_at":"2026-12-31T00:00:00Z","plan":2}}""")
        assertEquals("neo@example.org", tb?.username)
        assertNull(parseUser(DebridService.TORBOX, """{"success":false,"detail":"bad token"}"""))
        val ad = parseUser(DebridService.ALLDEBRID, """{"status":"success","data":{"user":{"username":"trinity","isPremium":true,"premiumUntil":1798675200}}}""")
        assertEquals("trinity", ad?.username)
        assertEquals(1798675200L * 1000, ad?.premiumUntilMs)
        assertNull(parseUser(DebridService.ALLDEBRID, """{"status":"error","error":{"code":"AUTH_BAD_APIKEY"}}"""))
        val pm = parseUser(DebridService.PREMIUMIZE, """{"status":"success","customer_id":"123","premium_until":1798675200}""")
        assertEquals("123", pm?.username)
        assertNull(parseUser(DebridService.REAL_DEBRID, "not json"))
    }

    @Test
    fun cacheAnswers() {
        assertEquals(setOf(h1), parseTorboxCached("""{"success":true,"data":{"${h1.uppercase()}":{"name":"x","size":1}}}""", listOf(h1, h2)))
        assertTrue(parseTorboxCached("""{"success":true,"data":null}""", listOf(h1)).isEmpty())
        assertEquals(setOf(h2), parsePremiumizeCached("""{"status":"success","response":[false,true],"filename":["",""]}""", listOf(h1, h2)))
        assertTrue(parsePremiumizeCached("""{"status":"error"}""", listOf(h1)).isEmpty())
    }

    @Test
    fun realDebridInfo() {
        val info = parseRdInfo(
            """{"id":"X","status":"downloaded","files":[{"id":1,"path":"/Show - 01.mkv","bytes":100,"selected":1},{"id":2,"path":"/Show - 02.mkv","bytes":200}],
               "links":["https://real-debrid.com/d/ABC"]}""",
        )
        assertNotNull(info)
        assertEquals("downloaded", info.status)
        assertEquals(listOf("1", "2"), info.files.map { it.id })
        assertEquals("Show - 01.mkv", info.files[0].name)
        assertEquals(listOf("https://real-debrid.com/d/ABC"), info.links)
        assertTrue("downloading" in RD_NOT_CACHED && "magnet_error" in RD_FAILED)
    }

    @Test
    fun torboxListing() {
        val list = """{"success":true,"data":[{"id":7,"hash":"$h2","download_present":false},{"id":9,"hash":"${h1.uppercase()}","download_present":true,
            "download_state":"cached","files":[{"id":0,"short_name":"Show - 01.mkv","size":100},{"id":1,"name":"Show/Show - 02.mkv","size":200}]}]}"""
        assertEquals("9", torboxExistingId(list, h1))
        assertNull(torboxExistingId(list, h2), "a torrent not present yet is not reused")
        val info = parseTorboxInfo(list, "9")
        assertNotNull(info)
        assertTrue(info.present)
        assertEquals(listOf("Show - 01.mkv", "Show/Show - 02.mkv"), info.files.map { it.name })
        assertNull(parseTorboxInfo(list, "404"))
    }

    @Test
    fun allDebridStatusAndTree() {
        assertEquals(4, parseAllDebridStatus("""{"status":"success","data":{"magnets":{"id":55,"statusCode":4}}}""", "55"))
        assertEquals(1, parseAllDebridStatus("""{"status":"success","data":{"magnets":[{"id":55,"statusCode":1}]}}""", "55"))
        val files = parseAllDebridFiles(
            """{"status":"success","data":{"magnets":[{"id":"55","files":[{"n":"Show","e":[{"n":"Show - 01.mkv","s":100,"l":"https://alldebrid.com/f/1"},
               {"n":"Sub","e":[{"n":"Show - 01.ass","s":1,"l":"https://alldebrid.com/f/2"}]}]}]}]}}""",
            "55",
        )
        assertEquals(listOf("Show/Show - 01.mkv", "Show/Sub/Show - 01.ass"), files.map { it.name })
        assertEquals("https://alldebrid.com/f/1", files[0].id)
    }

    @Test
    fun premiumizeContentKeepsOnlyHttpLinks() {
        val files = parsePremiumizeContent(
            """{"status":"success","content":[{"path":"Show/Show - 01.mkv","size":"100","link":"https://cdn.premiumize.me/1.mkv"},{"path":"x","size":1,"link":"ftp://no"}]}""",
        )
        assertEquals(1, files.size)
        assertEquals(100, files[0].size)
    }

    @Test
    fun bestVideoPrefersTheEpisodeThenTheLargest() {
        val files = listOf(
            RemoteFile("1", "Show/Show - 01.mkv", 500),
            RemoteFile("2", "Show/Show - 02.mkv", 400),
            RemoteFile("3", "Show/NCOP.mkv", 900),
            RemoteFile("4", "Show/readme.txt", 9_999),
        )
        assertEquals("2", selectBestVideo(files, 2)?.id)
        assertEquals("3", selectBestVideo(files, null)?.id)
        assertEquals("3", selectBestVideo(files, 40)?.id)
        assertNull(selectBestVideo(listOf(RemoteFile("x", "a.txt", 1)), 1))
        assertTrue(matchesEpisode("[Group] Show S01E05 [1080p].mkv", 5))
        assertTrue(matchesEpisode("Show - Episode 12.mkv", 12))
        assertTrue(!matchesEpisode("Show - 120.mkv", 12))
    }

    @Test
    fun magnetHashHexAndBase32() {
        assertEquals(h1, magnetHash("magnet:?xt=urn:btih:${h1.uppercase()}&dn=x"))
        // base32 of 20 zero bytes
        assertEquals("0".repeat(40), magnetHash("magnet:?xt=urn:btih:" + "A".repeat(32)))
        assertNull(magnetHash("magnet:?dn=nothing"))
    }

    @Test
    fun cachedStatusFollowsPreferenceOrder() {
        val c = SourceCandidate(extId = "nyaa", extName = "Nyaa", kind = SourceKind.TORRENT, label = "SubsPlease", torrent = TorrentInfo(infoHash = h1))
        val linked = listOf(DebridService.REAL_DEBRID, DebridService.TORBOX, DebridService.PREMIUMIZE)
        assertEquals(
            DebridStatus(DebridService.TORBOX, cached = true),
            DebridRepositoryImpl.debridStatusFor(c, linked, mapOf(h1 to setOf(DebridService.PREMIUMIZE, DebridService.TORBOX))),
        )
        assertEquals(DebridStatus(DebridService.TORBOX, cached = false), DebridRepositoryImpl.debridStatusFor(c, linked, emptyMap()))
        assertNull(DebridRepositoryImpl.debridStatusFor(c, listOf(DebridService.REAL_DEBRID), emptyMap()), "RD alone cannot tell")
        assertNull(DebridRepositoryImpl.debridStatusFor(c.copy(kind = SourceKind.WEB, torrent = null), linked, emptyMap()))
    }
}
