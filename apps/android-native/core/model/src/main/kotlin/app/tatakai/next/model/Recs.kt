// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.coroutines.flow.Flow
import kotlinx.serialization.Serializable

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.1): local recommendations ("Para ti") with a taste profile
// computed from the history and the list, explanations, "No me interesa" feedback and the affinity map (atlas). Everything is computed on
// the device (classic `src/core/recommendations/*`; no backend). Desktop counterpart: tk-core/recs.rs (worker b). Signatures FINAL;
// bodies: worker r (:core:discover, `discoverModule`).

/** Weighted preferences derived from what the user watched / listed. Weights are 0..1 (normalised). */
data class TasteProfile(
    val genres: Map<String, Float>,
    val studios: Map<String, Float>,
    /** Decade start year -> weight (`2010 -> 0.4`). */
    val eras: Map<Int, Float>,
    /** Mean score (0..100) of the titles the user rated or completed; null = unknown. */
    val preferredScore: Int?,
    /** Titles the profile was built from. */
    val sampleSize: Int,
)

enum class RecReasonKind { GENRE, STUDIO, ERA, SCORE, SIMILAR_TO }

/** "Porque te gusta Fantasy" / "Del estudio MAPPA" / "Parecida a Frieren". [label] is the user-facing value (genre, studio, title). */
data class RecReason(val kind: RecReasonKind, val label: String)

@Serializable
enum class RecFeedback { NOT_INTERESTED }

/** One recommendation: [affinity] 0..1 is shown as a percentage; [reasons] explain it (classic RecommendationsPage). */
data class RecommendationItem(val media: Media, val affinity: Float, val reasons: List<RecReason>)

/** One dot of the atlas: a 2D projection of the user's titles plus the recommendations; same [cluster] = same colour. */
data class AtlasPoint(val media: Media, val x: Float, val y: Float, val cluster: Int, val recommended: Boolean)

/** Implemented by worker r (`RecsRepositoryImpl`): profile from `HistoryRepository` + `ListsRepository`, candidates from `MetadataRepository`. */
interface RecsRepository {
    /** Null until there is at least one title in the history or the list. */
    suspend fun profile(): TasteProfile?

    /** Best first; never contains dismissed titles. */
    suspend fun forYou(limit: Int = 30): List<RecommendationItem>

    suspend fun feedback(ref: MediaRef, feedback: RecFeedback)

    fun dismissed(): Flow<List<MediaRef>>

    suspend fun undoDismiss(ref: MediaRef)

    suspend fun atlas(): List<AtlasPoint>
}
