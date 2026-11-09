// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent.store

import app.tatakai.next.model.torrent.TorrentSessionEntry
import org.junit.Rule
import org.junit.rules.TemporaryFolder
import java.io.File
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** parity wave A (worker q, torrent.session-history). */
class SessionHistoryStoreTest {
    @get:Rule
    val tmp = TemporaryFolder()

    private fun entry(id: String, at: Long, origin: String = "play") = TorrentSessionEntry(id = id, name = "n-$id", addedUnix = at, origin = origin)

    @Test
    fun recordMovesToTopRemovalIsStampedAndItPersists() {
        val file = File(tmp.root, "history.json")
        val store = SessionHistoryStore(file, max = 3)
        store.record(entry("a", 1))
        store.record(entry("b", 2))
        store.record(entry("a", 3, origin = "magnet"))
        assertEquals(listOf("a", "b"), store.entries.value.map { it.id })
        store.markRemoved("b", removedUnix = 9, downloaded = 100, uploaded = 5)
        assertEquals(9L, store.entries.value.first { it.id == "b" }.removedUnix)
        store.record(entry("b", 10))
        assertNull(store.entries.value.first { it.id == "b" }.removedUnix, "a re-added torrent is not removed")
        assertEquals(100L, store.entries.value.first { it.id == "b" }.downloadedBytes)
        store.record(entry("c", 11))
        store.record(entry("d", 12))
        assertEquals(3, store.entries.value.size)
        val reloaded = SessionHistoryStore(file, max = 3)
        assertEquals(store.entries.value.map { it.id }.toSet(), reloaded.entries.value.map { it.id }.toSet())
        reloaded.clear()
        assertTrue(SessionHistoryStore(file).entries.value.isEmpty())
    }
}
