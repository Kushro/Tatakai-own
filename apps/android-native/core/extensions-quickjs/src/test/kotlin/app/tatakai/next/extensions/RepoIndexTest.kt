// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.extensions

import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionRepo
import app.tatakai.next.model.InstalledExtension
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFailsWith
import kotlin.test.assertFalse
import kotlin.test.assertTrue

// parity wave B (16 §5 t.6c, worker t): `index.json` of extension repositories, versions and updates.
class RepoIndexTest {
    private val index = """
        {
          "name": "Community",
          "extensions": [
            { "id": "toko", "name": "Toko", "version": "1.2.0", "description": "Streams", "url": "packages/toko.kai", "notes": "Fixes" },
            { "id": "toko", "name": "Toko dup", "version": "9.9.9", "url": "x.kai" },
            { "id": "Bad Id", "version": "1.0.0", "url": "bad.kai" },
            { "id": "noversion", "url": "nv.kai" },
            { "id": "abs", "version": "0.1.0", "download": "https://cdn.example/abs.kai" }
          ]
        }
    """.trimIndent()

    @Test
    fun parsesEntriesResolvingRelativeUrls() {
        val p = RepoIndex.parse(index, "https://repo.example/tatakai/index.json")
        assertEquals("Community", p.name)
        assertEquals(listOf("toko", "abs"), p.entries.map { it.first.id })
        assertEquals("https://repo.example/tatakai/packages/toko.kai", p.entries[0].first.url)
        assertEquals("Fixes", p.entries[0].second)
        assertEquals("https://cdn.example/abs.kai", p.entries[1].first.url)
    }

    @Test
    fun bareArrayAndHostNameFallback() {
        val p = RepoIndex.parse("""[{"id":"a","version":"1","url":"a.kai"}]""", "https://repo.example/index.json")
        assertEquals("repo.example", p.name)
        assertEquals(1, p.entries.size)
        assertFailsWith<IllegalArgumentException> { RepoIndex.parse("<html>", "https://repo.example/") }
        assertFailsWith<IllegalArgumentException> { RepoIndex.parse("""{"name":"x"}""", "https://repo.example/") }
    }

    @Test
    fun versionsCompareNumerically() {
        assertTrue(Versions.isNewer("1.10.0", "1.9.3"))
        assertTrue(Versions.isNewer("v2.0", "1.99"))
        assertTrue(Versions.isNewer("1.0.0", "1.0.0-beta"))
        assertFalse(Versions.isNewer("1.0", "1.0.0"))
        assertFalse(Versions.isNewer("0.9", "1.0"))
    }

    @Test
    fun updatesPickTheNewestRepositoryEntry() {
        val installed = listOf(
            InstalledExtension("toko", "Toko", "1.0.0", "onlinestream", emptyList(), ExtensionCompat.FULL, true),
            InstalledExtension("same", "Same", "2.0.0", "onlinestream", emptyList(), ExtensionCompat.FULL, true),
        )
        val repoA = ExtensionRepo("https://a/index.json", "A", RepoIndex.parse(index, "https://a/index.json").entries.map { it.first }, 1L)
        val repoB = ExtensionRepo(
            "https://b/index.json", "B",
            RepoIndex.parse("""{"extensions":[{"id":"toko","version":"1.3.0","url":"t.kai"},{"id":"same","version":"2.0.0","url":"s.kai"}]}""", "https://b/index.json")
                .entries.map { it.first },
            1L,
        )
        val updates = findUpdates(installed, listOf(repoA, repoB), mapOf("https://b/index.json|toko" to "B notes"))
        assertEquals(1, updates.size)
        assertEquals("1.3.0", updates[0].availableVersion)
        assertEquals("https://b/t.kai", updates[0].url)
        assertEquals("B notes", updates[0].notes)
        val marked = markInstalled(repoA.entries, installed)
        assertTrue(marked.first { it.id == "toko" }.installed)
        assertTrue(marked.first { it.id == "toko" }.updateAvailable)
        assertFalse(marked.first { it.id == "abs" }.installed)
    }
}
