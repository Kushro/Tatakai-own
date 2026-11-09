// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.extensionserver.FakeServer.Companion.fail
import app.tatakai.next.extensionserver.FakeServer.Companion.health
import app.tatakai.next.extensionserver.FakeServer.Companion.json
import app.tatakai.next.extensionserver.FakeServer.Companion.ok
import app.tatakai.next.link.AesGcmKeyCipher
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import mockwebserver3.MockResponse
import okhttp3.OkHttpClient
import java.io.File
import java.nio.file.Files
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import javax.crypto.KeyGenerator
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertIs
import kotlin.test.assertTrue

class ExtensionServerRepositoryTest {
    private val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    private val dir: File = Files.createTempDirectory("tatakai-extserver-test").toFile()
    private val storeFile = File(dir, "extension-servers.bin")
    private val http = OkHttpClient.Builder().followRedirects(false).build()
    private val client = ExtensionServerClient(http)

    private fun store() = ExtensionServerStore(storeFile, AesGcmKeyCipher { secret })

    private fun repo(): ExtensionServerRepositoryImpl {
        var n = 0
        return ExtensionServerRepositoryImpl(store(), client, deviceLabel = "Tatakai Next (Test)", newId = { "id-${++n}" })
    }

    private val media = Media(
        ref = MediaRef(anilistId = 21, malId = 21), kind = MediaKind.ANIME, title = "One Piece", titles = mapOf("native" to "ワンピース"),
        cover = null, banner = null, color = null, synopsis = null, genres = emptyList(), episodes = null, status = null, season = null,
        year = null, score = null, sourceProvider = "anilist",
    )

    private fun pairingServer(): FakeServer = FakeServer { req ->
        when (req.url.encodedPath) {
            "/api/server/v1/health" -> ok(health())
            "/api/server/v1/auth/pair" -> ok("""{"token":"secret-token-xyz","role":"viewer","deviceId":"d-1"}""")
            "/api/server/v1/auth/logout" -> ok("""{"success":true}""")
            else -> fail(404, "route_not_found")
        }
    }

    @Test
    fun pairingStoresTheConnectionSealed() = pairingServer().use { s ->
        runBlocking {
            val r = repo()
            val conn = r.connectWithLink(PairingLink(s.origin, "code-1"))
            assertEquals("srv-1", conn.serverId)
            assertEquals("Tatakai (desk)", conn.name)
            assertEquals(s.origin, conn.url)
            assertEquals(listOf(conn), r.servers.value)
            assertFalse("secret-token-xyz" in conn.toString())
            // Sealed at rest: neither the token nor the URL is readable in the file.
            val raw = storeFile.readBytes().decodeToString()
            assertFalse("secret-token-xyz" in raw)
            assertFalse("127.0.0.1" in raw)
            // A new repository reads it back.
            assertEquals(listOf(conn), repo().enabledServers())
            // Pairing again with the same server keeps the id (no duplicates).
            val again = r.connectWithLink(PairingLink(s.origin, "code-2"))
            assertEquals(conn.id, again.id)
            assertEquals(1, r.servers.value.size)
        }
    }

    @Test
    fun wrongKeyOrGarbageReadsAsNoServers() = pairingServer().use { s ->
        runBlocking {
            repo().connectWithLink(PairingLink(s.origin, "c"))
            val other = ExtensionServerStore(storeFile, AesGcmKeyCipher { KeyGenerator.getInstance("AES").apply { init(256) }.generateKey() })
            assertTrue(other.read().isEmpty())
            storeFile.writeText("garbage")
            assertTrue(store().read().isEmpty())
        }
    }

    @Test
    fun incompatibleOrUnconfiguredServersAreRefused() {
        FakeServer { ok("""{"protocolVersion":2,"serverId":"s","name":"x"}""") }.use { s ->
            val e = assertFailsWith<ExtensionServerException> { runBlocking { repo().connectWithLink(PairingLink(s.origin, "c")) } }
            assertEquals(ExtensionServerException.INCOMPATIBLE, e.code)
        }
        FakeServer { ok("""{"protocolVersion":1,"serverId":null,"setupRequired":true}""") }.use { s ->
            val e = assertFailsWith<ExtensionServerException> { runBlocking { repo().connectWithPassword(s.origin, "pw") } }
            assertEquals(ExtensionServerException.SETUP_REQUIRED, e.code)
        }
    }

    @Test
    fun passwordLoginNormalizesTheUrlAndReportsPairingRequired() = FakeServer { req ->
        if (req.url.encodedPath.endsWith("/health")) ok(health()) else fail(401, "pairing_required")
    }.use { s ->
        val e = assertFailsWith<ExtensionServerException> { runBlocking { repo().connectWithPassword(s.origin + "/", "") } }
        assertEquals(ExtensionServerException.PAIRING_REQUIRED, e.code)
        val bad = assertFailsWith<ExtensionServerException> { runBlocking { repo().connectWithPassword("http://example.org", "x") } }
        assertEquals(ExtensionServerException.INSECURE_HOST, bad.code)
        val path = assertFailsWith<ExtensionServerException> { runBlocking { repo().connectWithPassword(s.origin + "/server/", "x") } }
        assertEquals(ExtensionServerException.INVALID_URL, path.code)
    }

    @Test
    fun removeLogsOutAndDisableHidesTheServerFromResolution() {
        val logouts = AtomicInteger()
        FakeServer { req ->
            when (req.url.encodedPath) {
                "/api/server/v1/health" -> ok(health())
                "/api/server/v1/auth/pair" -> ok("""{"token":"t","role":"viewer","deviceId":"d"}""")
                "/api/server/v1/auth/logout" -> ok("{}").also { logouts.incrementAndGet() }
                else -> fail(404, "route_not_found")
            }
        }.use { s ->
            runBlocking {
                val r = repo()
                val conn = r.connectWithLink(PairingLink(s.origin, "c"))
                r.setEnabled(conn.id, false)
                assertTrue(r.enabledServers().isEmpty())
                assertTrue(ServerCandidateSource(r, client).candidates(media, 1).isEmpty())
                r.remove(conn.id)
                assertTrue(r.servers.value.isEmpty())
                assertTrue(store().read().isEmpty())
                assertEquals(1, logouts.get())
            }
        }
    }

    @Test
    fun candidateSourceMergesTheLatestSnapshotAndCaches() {
        val resolves = AtomicInteger()
        FakeServer { req ->
            when (req.url.encodedPath) {
                "/api/server/v1/health" -> ok(health())
                "/api/server/v1/auth/pair" -> ok("""{"token":"t","role":"viewer","deviceId":"d"}""")
                "/api/server/v1/resolve" -> {
                    resolves.incrementAndGet()
                    val o = "http://127.0.0.1:${req.url.port}"
                    FakeServer.ndjson(
                        listOf(
                            """{"sources":[],"statuses":[{"provider":"srv-1/a","status":"loading"}]}""",
                            """{"sources":[{"url":"$o/api/server/v1/media/1","sourceType":"hls","sourceName":"S1","providerKey":"srv-1/a","providerName":"A · Desk"}],"statuses":[{"provider":"srv-1/a","status":"ok","count":1}]}""",
                        ),
                    )
                }
                else -> fail(404, "route_not_found")
            }
        }.use { s ->
            runBlocking {
                val r = repo()
                val conn = r.connectWithLink(PairingLink(s.origin, "c"))
                val source = ServerCandidateSource(r, client)
                val list = source.candidates(media, 5)
                assertEquals(listOf("server:srv-1/a"), list.map { it.extId })
                assertEquals("A · Desk", list.single().extName)
                assertIs<ServerStatus.Online>(r.status.value[conn.id])
                source.candidates(media, 5)
                assertEquals(1, resolves.get(), "complete non-empty results are reused")
                // The request carries what the classic client sends.
                s.take(); s.take()
                val body = Json.parseToJsonElement(s.take().body!!.utf8()).toString()
                assertTrue("\"anilistId\":21" in body && "\"episode\":5" in body && "\"title\":\"One Piece\"" in body, body)
            }
        }
    }

    @Test
    fun aSlowServerContributesWhatArrivedWithinItsBudget() {
        FakeServer { req ->
            when (req.url.encodedPath) {
                "/api/server/v1/health" -> ok(health())
                "/api/server/v1/auth/pair" -> ok("""{"token":"t","role":"viewer","deviceId":"d"}""")
                "/api/server/v1/resolve" -> {
                    val o = "http://127.0.0.1:${req.url.port}"
                    val first = """{"sources":[{"url":"$o/api/server/v1/media/1","sourceType":"hls","sourceName":"Fast","providerKey":"srv-1/a"}],"statuses":[{"provider":"srv-1/a","status":"ok"},{"provider":"srv-1/b","status":"loading"}]}""" + "\n"
                    val second = """{"sources":[],"statuses":[]}""" + "\n"
                    // The first line goes out at once; the rest only after 5 s.
                    MockResponse.Builder().code(200).addHeader("Content-Type", "application/x-ndjson")
                        .body(first + second).throttleBody(first.toByteArray().size.toLong(), 5, TimeUnit.SECONDS).build()
                }
                else -> fail(404, "route_not_found")
            }
        }.use { s ->
            runBlocking {
                val r = repo()
                r.connectWithLink(PairingLink(s.origin, "c"))
                val started = System.nanoTime()
                val list = ServerCandidateSource(r, client, budgetMs = 1_000).candidates(media, 1)
                val elapsedMs = (System.nanoTime() - started) / 1_000_000
                assertEquals(listOf("Fast"), list.map { it.label })
                assertTrue(elapsedMs < 4_000, "bounded by the budget, took $elapsedMs ms")
            }
        }
    }

    @Test
    fun aDownServerContributesNothingAndIsMarkedOffline() {
        val r = repo()
        val server = pairingServer()
        val conn = runBlocking { r.connectWithLink(PairingLink(server.origin, "c")) }
        server.close() // the PC goes away: connection refused
        runBlocking {
            assertTrue(ServerCandidateSource(r, client).candidates(media, 1).isEmpty())
            assertEquals(ServerStatus.Offline, r.status.value[conn.id])
        }
    }

    @Test
    fun revokedTokenMarksTheServerUnauthorized() = FakeServer { req ->
        when (req.url.encodedPath) {
            "/api/server/v1/health" -> ok(health())
            "/api/server/v1/auth/pair" -> ok("""{"token":"t","role":"viewer","deviceId":"d"}""")
            else -> json("""{"success":false,"error":{"code":"device_revoked"}}""", 401)
        }
    }.use { s ->
        runBlocking {
            val r = repo()
            val conn = r.connectWithLink(PairingLink(s.origin, "c"))
            assertTrue(ServerCandidateSource(r, client).candidates(media, 1).isEmpty())
            assertEquals(ServerStatus.Unauthorized, r.status.value[conn.id])
        }
    }
}
