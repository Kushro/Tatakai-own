// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.PickedSourceRow
import app.tatakai.next.database.SourceStatRow
import app.tatakai.next.database.SourceStatsDao
import app.tatakai.next.model.INITIAL_RELIABILITY
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class SourceStatsStoreTest {
    private class MemoryDao : SourceStatsDao() {
        val stats = HashMap<Pair<String, String>, SourceStatRow>()
        val picks = HashMap<Pair<String, Int>, PickedSourceRow>()
        override suspend fun stat(extId: String, label: String) = stats[extId to label]
        override suspend fun putStat(row: SourceStatRow) { stats[row.extId to row.label] = row }
        override suspend fun picked(mediaId: String, episode: Int) = picks[mediaId to episode]?.key
        override suspend fun putPicked(row: PickedSourceRow) { picks[row.mediaId to row.episode] = row }
        override suspend fun clearPicked(mediaId: String, episode: Int) { picks.remove(mediaId to episode) }
        override suspend fun clearStats() = stats.clear()
    }

    @Test
    fun reliabilityStartsAtTheInitialValueAndMovesWithResults() = runTest {
        val dao = MemoryDao()
        val store = SourceStatsStoreImpl(dao) { 42L }
        assertEquals(INITIAL_RELIABILITY, store.reliability("ext", "Alpha"))
        assertTrue(abs(store.recordSourceResult("ext", "Alpha", true) - 0.84f) < 1e-6f)
        assertTrue(abs(store.recordSourceResult("ext", "Alpha", false) - 0.672f) < 1e-5f)
        assertTrue(abs(store.reliability("ext", "Alpha") - 0.672f) < 1e-5f)
        assertEquals(1, dao.stats["ext" to "Alpha"]!!.okCount)
        assertEquals(1, dao.stats["ext" to "Alpha"]!!.failCount)
        assertEquals(INITIAL_RELIABILITY, store.reliability("ext", "Beta"))
    }

    @Test
    fun pickedSourcePerEpisode() = runTest {
        val store = SourceStatsStoreImpl(MemoryDao())
        assertNull(store.pickedSource("21", 3))
        store.setPickedSource("21", 3, "ext|web|Alpha")
        assertEquals("ext|web|Alpha", store.pickedSource("21", 3))
        assertNull(store.pickedSource("21", 4))
        store.clearPickedSource("21", 3)
        assertNull(store.pickedSource("21", 3))
    }
}
