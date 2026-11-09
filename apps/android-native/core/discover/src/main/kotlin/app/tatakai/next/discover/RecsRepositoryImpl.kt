// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AtlasPoint
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.RecFeedback
import app.tatakai.next.model.RecommendationItem
import app.tatakai.next.model.RecsRepository
import app.tatakai.next.model.TasteProfile
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import java.io.File

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r; checklist home.recs.*, home.because-you-watched):
 * [RecsRepository] computed on the device (port of the classic `src/core/recommendations/*`): the profile weighs genres / studios /
 * decades of the history (recency-weighted) and of the list (status + score), candidates come from `MetadataRepository` (trending,
 * popular, `recommendations(ref)` of the top 5 titles), scored by cosine affinity minus already-seen / dismissed; the atlas is a 2D
 * projection (genre vectors → PCA-like power iteration, k-means clusters) of seen + recommended titles. Dismissed refs persist in
 * `storeDir/dismissed.json`; the last profile / result is cached 6 h in `storeDir/recs.json`. Pure logic goes in a `RecsEngine` object
 * with JVM tests (fixtures of fake histories).
 *
 * Stub: no profile, no recommendations.
 */
class RecsRepositoryImpl(
    private val metadata: MetadataRepository,
    private val history: HistoryRepository,
    private val lists: ListsRepository? = null,
    private val storeDir: File,
) : RecsRepository, AppDataWiper {
    private val dismissed = MutableStateFlow<List<MediaRef>>(emptyList())

    override suspend fun profile(): TasteProfile? = null

    override suspend fun forYou(limit: Int): List<RecommendationItem> = emptyList()

    override suspend fun feedback(ref: MediaRef, feedback: RecFeedback) {
        if (feedback == RecFeedback.NOT_INTERESTED && ref !in dismissed.value) dismissed.value = dismissed.value + ref
    }

    override fun dismissed(): Flow<List<MediaRef>> = dismissed

    override suspend fun undoDismiss(ref: MediaRef) {
        dismissed.value = dismissed.value - ref
    }

    override suspend fun atlas(): List<AtlasPoint> = emptyList()

    override suspend fun wipe() {
        dismissed.value = emptyList()
        storeDir.deleteRecursively()
    }

    companion object {
        const val DIR_NAME: String = "recs"
    }
}
