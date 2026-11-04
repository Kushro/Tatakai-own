// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ExtLogLevel
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceStatsStore
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import java.io.ByteArrayOutputStream
import java.io.File
import java.nio.file.Files
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNotNull
import kotlin.test.assertTrue

/**
 * resolveCandidates / reportSourceResult / detail / logs of [ExtensionRepositoryImpl] over a scripted engine: a custom extension with an
 * explicit `torrent` route (Toko-like) answers both routes; the candidates come back ranked, with stored reliabilities and the pick.
 */
class ExtensionRepositoryCandidatesTest {
    private class Session(val handlers: Map<String, (List<JsonElement>) -> JsonElement>) : JsSession {
        override suspend fun hasMethod(name: String) = name in handlers
        override suspend fun invoke(method: String, args: List<JsonElement>, collectChunks: Boolean) =
            JsInvocation(handlers.getValue(method)(args))
        override fun close() = Unit
    }

    private val streamBody = Json.parseToJsonElement(
        """{ "sources": [
            { "providerName": "alpha", "server": "Alpha", "sourceType": "hls", "url": "https://cdn.toko.example/a.m3u8", "quality": "1080p", "audioLanguage": "ja" },
            { "providerName": "embed", "sourceType": "embed", "url": "https://embed.example/e" }
        ]}""",
    )
    private val torrentBody = Json.parseToJsonElement(
        """{ "sources": [
            { "source": "nyaa", "providerName": "nyaa", "sourceType": "torrent", "quality": "1080p",
              "url": "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=x",
              "torrentTitle": "[SubsPlease] Show - 05 (1080p) [A1B2C3D4].mkv", "releaseGroup": "SubsPlease", "seeders": 812, "leechers": 41 }
        ]}""",
    )

    private val engine = object : JsEngine {
        override suspend fun open(manifest: ExtensionManifest, bundle: String, host: JsHost): JsSession =
            Session(mapOf("streamAll" to { _ -> streamBody }, "torrentAll" to { _ -> torrentBody }))
    }

    private object NoHost : JsHost {
        override val networkPolicy = NetworkPolicy.NONE
        override suspend fun fetch(request: HostFetchRequest): HostFetchResponse = throw ExtError.PermissionDenied()
        override fun log(level: String, message: String) = Unit
    }

    private class Stats : SourceStatsStore {
        val recorded = mutableListOf<Triple<String, String, Boolean>>()
        override suspend fun reliability(extId: String, label: String) = if (label == "Alpha") 0.1f else 0.95f
        override suspend fun recordSourceResult(extId: String, label: String, ok: Boolean): Float {
            recorded += Triple(extId, label, ok)
            return 0.5f
        }
        override suspend fun pickedSource(mediaId: String, episode: Int) = if (mediaId == "21" && episode == 5) "x.toko|web|Alpha" else null
        override suspend fun setPickedSource(mediaId: String, episode: Int, key: String) = Unit
        override suspend fun clearPickedSource(mediaId: String, episode: Int) = Unit
    }

    private class Settings : SettingsRepository {
        override val settings: StateFlow<AppSettings> = MutableStateFlow(AppSettings())
        override suspend fun update(transform: (AppSettings) -> AppSettings) = Unit
    }

    private fun kai(dir: File): File {
        val manifest = """{"id":"x.toko","name":"Toko","version":"1.0.0","type":"custom","description":"All in one",
            "permissions":["network:domain:toko.example","network:domain:sololatino.net"],
            "browserDomains":["sololatino.net","evil.example"],
            "apiServer":{"routes":["sources","stream","torrent"]}}"""
        val bundle = "module.exports = class { async streamAll(o, cb) {} async torrentAll(o, cb) {} };"
        val out = ByteArrayOutputStream()
        ZipOutputStream(out).use { zip ->
            zip.putNextEntry(ZipEntry("manifest.json")); zip.write(manifest.toByteArray()); zip.closeEntry()
            zip.putNextEntry(ZipEntry("bundle.js")); zip.write(bundle.toByteArray()); zip.closeEntry()
        }
        return File(dir, "toko.kai").apply { writeBytes(out.toByteArray()) }
    }

    private val media = Media(
        ref = MediaRef(anilistId = 21), kind = MediaKind.ANIME, title = "Show", titles = emptyMap(), cover = null, banner = null,
        color = null, synopsis = null, genres = emptyList(), episodes = 12, status = null, season = null, year = null, score = null,
        sourceProvider = "anilist",
    )

    @Test
    fun candidatesAreRankedWithReliabilityAndPick() = runBlocking {
        val root = Files.createTempDirectory("tatakai-ext-cand").toFile()
        try {
            val stats = Stats()
            val logs = ExtensionLogStore()
            val repo = ExtensionRepositoryImpl(
                rootDir = File(root, "installed"),
                openUri = { File(it).inputStream() },
                host = ExtensionHost(engine, { NoHost }),
                io = Dispatchers.Unconfined,
                settings = Settings(),
                stats = stats,
                logStore = logs,
            )
            val ext = repo.installFromUri(kai(root).path).getOrThrow()
            assertEquals(listOf("sololatino.net"), ext.browserDomains)
            assertEquals("All in one", ext.description)

            val ranked = repo.resolveCandidates(media, 5)
            assertEquals(2, ranked.size)
            assertEquals(setOf(SourceKind.WEB, SourceKind.TORRENT), ranked.map { it.candidate.kind }.toSet())
            assertTrue(ranked.first().recommended && ranked.drop(1).none { it.recommended })
            val web = ranked.single { it.candidate.kind == SourceKind.WEB }
            assertEquals(0.1f, web.candidate.reliability)
            assertTrue(web.picked)
            val torrent = ranked.single { it.candidate.kind == SourceKind.TORRENT }
            assertEquals("SubsPlease", torrent.candidate.label)
            assertEquals("abcdef0123456789abcdef0123456789abcdef01", torrent.candidate.torrent!!.infoHash)
            assertTrue(ranked.zipWithNext().all { (a, b) -> a.score >= b.score })

            repo.reportSourceResult(web.key, ok = false)
            assertEquals(listOf(Triple("x.toko", "Alpha", false)), stats.recorded)

            val detail = assertNotNull(repo.detail("x.toko"))
            assertEquals(listOf("toko.example", "sololatino.net"), detail.domains)
            assertEquals(listOf("stream", "torrent"), detail.sourceRoutes)
            assertEquals("custom", detail.kind)
            assertTrue(detail.hash.contains('…') && detail.hash.length == 9)
            assertNotNull(detail.installedAtMs)

            val lines = repo.logs("x.toko").first()
            assertTrue(lines.any { it.op == "install" && it.level == ExtLogLevel.OK })
            assertTrue(lines.any { it.op == "stream" && it.message == "1 sources" })
            assertTrue(lines.any { it.op == "torrent" && it.message == "1 sources" })
            assertTrue(lines.any { it.op == "source" && it.level == ExtLogLevel.WARN })
            assertTrue(lines.any { it.op == "browser" && it.level == ExtLogLevel.WARN }) // evil.example ignored

            repo.setEnabled("x.toko", false)
            assertTrue(repo.resolveCandidates(media, 5).isEmpty())
            assertFalse(repo.installed.value.single().enabled)
        } finally {
            root.deleteRecursively() // temp dir created above
        }
    }
}
