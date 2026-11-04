// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import app.tatakai.next.database.PickedSourceRow
import app.tatakai.next.database.SourceStatRow
import app.tatakai.next.database.SourceStatsDao
import app.tatakai.next.model.INITIAL_RELIABILITY
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.release.updateReliability
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/**
 * Room-backed [SourceStatsStore] (tables `source_stats` / `picked_source`, docs/redesign/12 §3.1): desktop LocalStore reliability and
 * picked source. Reliability updates are serialized by a mutex so two concurrent results never lose one another.
 */
class SourceStatsStoreImpl(
    private val dao: SourceStatsDao,
    private val clock: () -> Long = System::currentTimeMillis,
) : SourceStatsStore {
    private val lock = Mutex()

    override suspend fun reliability(extId: String, label: String): Float =
        dao.stat(extId, label)?.reliability?.takeIf { it.isFinite() } ?: INITIAL_RELIABILITY

    override suspend fun recordSourceResult(extId: String, label: String, ok: Boolean): Float = lock.withLock {
        val prev = dao.stat(extId, label)
        val next = updateReliability(prev?.reliability ?: INITIAL_RELIABILITY, ok)
        dao.putStat(
            SourceStatRow(
                extId = extId,
                label = label,
                reliability = next,
                okCount = (prev?.okCount ?: 0) + if (ok) 1 else 0,
                failCount = (prev?.failCount ?: 0) + if (ok) 0 else 1,
                updatedAtMs = clock(),
            ),
        )
        next
    }

    override suspend fun pickedSource(mediaId: String, episode: Int): String? = dao.picked(mediaId, episode)

    override suspend fun setPickedSource(mediaId: String, episode: Int, key: String) =
        dao.putPicked(PickedSourceRow(mediaId, episode, key, clock()))

    override suspend fun clearPickedSource(mediaId: String, episode: Int) = dao.clearPicked(mediaId, episode)
}
