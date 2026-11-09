// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ProviderBookmark
import mockwebserver3.Dispatcher
import mockwebserver3.MockResponse
import mockwebserver3.MockWebServer
import mockwebserver3.RecordedRequest
import okhttp3.OkHttpClient
import org.junit.Rule
import org.junit.rules.TemporaryFolder
import java.io.File
import java.io.IOException
import kotlin.test.Test
import kotlin.test.assertContentEquals
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

// F6 (parity wave B tests, 16 §5 t.3): the catalog.json store of ExtensionRepositoryImpl (repositories, bookmarks, kill-switch,
// non-secret preferences) with TemporaryFolder, the stored <-> model mappings, and the size-capped GET used for indexes / bundles.
class CatalogStoreTest {
    @get:Rule
    val tmp = TemporaryFolder()

    @Test
    fun missingOrCorruptFileReadsAsEmptyState() {
        val file = File(tmp.root, "ext/catalog.json")
        val store = CatalogStore(file)
        assertEquals(CatalogState(), store.read())
        file.parentFile!!.mkdirs()
        file.writeText("{broken")
        assertEquals(CatalogState(), store.read())
    }

    @Test
    fun writeReadRoundTripKeepsEverySection() {
        val file = File(tmp.root, "ext/catalog.json")
        val store = CatalogStore(file)
        val state = CatalogState(
            repos = listOf(
                StoredRepo(
                    url = "https://repo.example/index.json",
                    name = "Example",
                    entries = listOf(StoredRepoEntry("toko", "Toko", "1.2.0", "desc", "https://repo.example/toko.kai", notes = "fixes")),
                    fetchedAtMs = 42,
                    error = null,
                ),
            ),
            bookmarks = listOf(StoredBookmark("toko", "main", "item-1", "Title", poster = null, completed = true, addedAtMs = 7)),
            killSwitchPrevious = listOf("toko", "latino"),
            preferences = mapOf("toko" to mapOf("quality" to "1080")),
        )
        store.write(state)
        assertTrue(file.isFile)
        assertFalse(File(file.parentFile, "catalog.json.tmp").exists(), "the temp file is renamed away")
        assertEquals(state, store.read())
        assertEquals(state, CatalogStore(file).read(), "a new instance reads the same file")

        store.write(state.copy(killSwitchPrevious = null))
        assertNull(store.read().killSwitchPrevious, "overwriting replaces the previous state")

        store.delete()
        assertFalse(file.exists())
        assertEquals(CatalogState(), store.read())
    }

    @Test
    fun unknownKeysAreIgnoredForForwardCompatibility() {
        val file = File(tmp.root, "catalog.json")
        file.writeText("""{"repos":[],"bookmarks":[],"future":{"x":1},"preferences":{"a":{"k":"v"}}}""")
        assertEquals(mapOf("a" to mapOf("k" to "v")), CatalogStore(file).read().preferences)
    }

    @Test
    fun storedModelsMapBothWays() {
        val repo = StoredRepo(
            url = "https://r/index.json", name = "R",
            entries = listOf(StoredRepoEntry("a", "A", "1.0", url = "https://r/a.kai")), fetchedAtMs = 5, error = "HTTP 500",
        ).toModel()
        assertEquals("https://r/index.json", repo.url)
        assertEquals(listOf("a"), repo.entries.map { it.id })
        assertEquals("https://r/a.kai", repo.entries.single().url)
        assertEquals(5L, repo.fetchedAtMs)
        assertEquals("HTTP 500", repo.error)

        val bookmark = ProviderBookmark("ext", "src", "item", "Title", poster = "p", completed = true, addedAtMs = 9)
        assertEquals(bookmark, bookmark.toStored().toModel())
    }

    private fun server(handler: (RecordedRequest) -> MockResponse): MockWebServer = MockWebServer().apply {
        dispatcher = object : Dispatcher() {
            override fun dispatch(request: RecordedRequest): MockResponse = handler(request)
        }
        start()
    }

    @Test
    fun httpGetBytesReturnsTheBodyAndSendsAccept() {
        var accept: String? = null
        val s = server { req ->
            accept = req.headers["Accept"]
            MockResponse.Builder().code(200).body("hello").build()
        }
        try {
            val bytes = httpGetBytes(OkHttpClient(), s.url("/index.json").toString(), maxBytes = 1024, accept = "application/json")
            assertContentEquals("hello".toByteArray(), bytes)
            assertEquals("application/json", accept)
        } finally {
            s.close()
        }
    }

    @Test
    fun httpGetBytesRejectsErrorsOversizeAndBadUrls() {
        val s = server { req ->
            when (req.url.encodedPath) {
                "/big" -> MockResponse.Builder().code(200).body("x".repeat(2048)).build()
                else -> MockResponse.Builder().code(404).body("nope").build()
            }
        }
        try {
            val client = OkHttpClient()
            assertFailsWith<IOException> { httpGetBytes(client, s.url("/missing").toString(), maxBytes = 1024) }
            assertFailsWith<IOException> { httpGetBytes(client, s.url("/big").toString(), maxBytes = 1024) }
            assertContentEquals(ByteArray(2048) { 'x'.code.toByte() }, httpGetBytes(client, s.url("/big").toString(), maxBytes = 2048))
            assertFailsWith<IOException> { httpGetBytes(client, "not a url", maxBytes = 10) }
            assertFailsWith<IOException> { httpGetBytes(client, "ftp://host/file", maxBytes = 10) }
        } finally {
            s.close()
        }
    }
}
