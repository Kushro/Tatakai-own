// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensionserver

import app.tatakai.next.extensionserver.FakeServer.Companion.fail
import app.tatakai.next.extensionserver.FakeServer.Companion.health
import app.tatakai.next.extensionserver.FakeServer.Companion.ok
import app.tatakai.next.link.AesGcmKeyCipher
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import okhttp3.OkHttpClient
import java.io.File
import java.nio.file.Files
import java.util.Collections
import javax.crypto.KeyGenerator
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

// parity wave B (16 §3.4, worker t; ext.ecosystems.via-server / ext.server-client.fallback): the PC servers' custom sources.
class ServerCustomSourcesTest {
    private val secret = KeyGenerator.getInstance("AES").apply { init(256) }.generateKey()
    private val dir: File = Files.createTempDirectory("tatakai-custom-test").toFile()
    private val client = ExtensionServerClient(OkHttpClient.Builder().followRedirects(false).build())

    private fun repo() = ExtensionServerRepositoryImpl(
        ExtensionServerStore(File(dir, "s.bin"), AesGcmKeyCipher { secret }), client, deviceLabel = "Test", newId = { "id-1" },
    )

    private val ops = Collections.synchronizedList(mutableListOf<String>())

    private fun server(): FakeServer = FakeServer { req ->
        when (req.url.encodedPath) {
            "/api/server/v1/health" -> ok(health())
            "/api/server/v1/auth/pair" -> ok("""{"token":"tok","role":"viewer","deviceId":"d-1"}""")
            "/api/server/v1/sources" -> ok(
                """[{"id":"mihon/mangadex~en","extensionId":"mihon.mangadex","name":"MangaDex","ecosystem":"mihon","kind":"read"},
                    {"id":"sora/off","extensionId":"sora.off","name":"Off","kind":"watch","available":false}]""",
            )
            "/api/server/v1/invoke" -> {
                val body = Json.parseToJsonElement(req.body?.utf8() ?: "{}") as JsonObject
                val op = (body["op"] as JsonPrimitive).content
                ops += op
                when (op) {
                    // The Node server refuses the extension-host route names and wants its data ops.
                    "custom/home", "custom/info" -> fail(400, "data_operation_required")
                    "browse" -> ok("""{"items":[{"id":"t1","name":"Title One","poster":"https://i/1.jpg"}]}""")
                    "info" -> ok("""{"id":"t1","name":"Title One"}""")
                    "chapters" -> ok("""{"chapters":[{"id":"c1","chapter":1},{"id":"c2","chapter":2}]}""")
                    else -> fail(404, "not_found")
                }
            }
            else -> fail(404, "route_not_found")
        }
    }

    @Test
    fun listsAvailableSourcesAndFallsBackToDataOps() = server().use { s ->
        runBlocking {
            val r = repo()
            r.connectWithLink(PairingLink(s.origin, "code"))
            val custom = ServerCustomSources(r, client)
            val refs = custom.customSources()
            assertEquals(1, refs.size)
            val ref = refs.single()
            assertEquals("server:srv-1/mihon.mangadex", ref.extensionId)
            assertEquals("read", ref.kind)
            assertTrue(custom.owns(ref.extensionId))
            val rows = custom.home(ref.extensionId, ref.sourceId)
            assertEquals(listOf("t1"), rows.single().items.map { it.id })
            val item = custom.info(ref.extensionId, ref.sourceId, "t1")
            assertEquals(listOf("c1", "c2"), item?.entries?.map { it.id })
            assertEquals(listOf("custom/home", "browse", "custom/info", "info", "chapters"), ops.toList())
        }
    }

    @Test
    fun aServerThatIsDownContributesNothing() {
        val s = server()
        runBlocking {
            val r = repo()
            r.connectWithLink(PairingLink(s.origin, "code"))
            s.close()
            val custom = ServerCustomSources(r, client, listTimeoutMs = 2_000)
            assertTrue(custom.customSources().isEmpty())
        }
    }
}
