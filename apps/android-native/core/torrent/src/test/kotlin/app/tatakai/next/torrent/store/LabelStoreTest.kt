// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.store

import app.tatakai.next.model.torrent.TorrentLabel
import java.io.File
import java.nio.file.Files
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class LabelStoreTest {

    private fun tempFile(): File = Files.createTempDirectory("labels").toFile().resolve("labels.json")

    @Test
    fun `entries survive a reload`() {
        val file = tempFile()
        val store = LabelStore(file)
        store.upsert(
            StoredTorrent(
                id = "a".repeat(40),
                name = "[Group] Show - 01.mkv",
                label = TorrentLabel(mediaId = "123", title = "Show", episode = 1, poster = null),
                persist = true,
                seed = true,
                addedUnix = 42,
                selected = listOf(0),
            ),
        )
        store.update("a".repeat(40)) { it.copy(progress = 0.5) }

        val reloaded = LabelStore(file)
        val t = reloaded["a".repeat(40)]!!
        assertEquals("Show", t.label?.title)
        assertTrue(t.persist)
        assertTrue(t.seed)
        assertEquals(42, t.addedUnix)
        assertEquals(listOf(0), t.selected)
        assertEquals(0.5, t.progress)
    }

    @Test
    fun `remove and unknown updates`() {
        val store = LabelStore(tempFile())
        store.upsert(StoredTorrent(id = "b".repeat(40)))
        assertNull(store.update("c".repeat(40)) { it.copy(persist = true) })
        store.remove("b".repeat(40))
        assertTrue(store.torrents.value.isEmpty())
    }

    @Test
    fun `corrupt file starts empty`() {
        val file = tempFile()
        file.parentFile.mkdirs()
        file.writeText("{not json")
        assertTrue(LabelStore(file).torrents.value.isEmpty())
    }
}
