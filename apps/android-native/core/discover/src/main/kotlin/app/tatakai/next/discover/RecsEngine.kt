// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.AtlasPoint
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.RecReason
import app.tatakai.next.model.RecReasonKind
import app.tatakai.next.model.RecommendationItem
import app.tatakai.next.model.TasteProfile
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.max
import kotlin.math.sqrt

/**
 * Parity wave B (16 §3.1, worker r; checklist home.recs.*): the pure part of the on-device recommendations, a port of the classic
 * `src/core/recommendations/{scoring,tasteProfile,atlasEmbedding}.ts` reduced to what the Next model carries (genres, studios, decade,
 * score). No I/O: [RecsRepositoryImpl] feeds it the history / list signals and the candidate titles.
 */
object RecsEngine {
    /** What the user did with one title. [affinity] in -1..1 ([affinityOf]); [studios] may be empty when unknown. */
    data class Signal(val media: Media, val affinity: Float, val userScore: Int? = null, val studios: List<String> = emptyList())

    /** A title that could be recommended; [similarTo] = the seen title whose AniList recommendations produced it. */
    data class Candidate(val media: Media, val studios: List<String> = emptyList(), val similarTo: String? = null)

    /** Classic `computeAffinity`: status base value, user score (0..100) centred on 55, completion floor. */
    fun affinityOf(status: ListStatus?, score: Int?, completed: Boolean): Float {
        var sum = 0f
        var parts = 0
        when (status) {
            ListStatus.COMPLETED -> 1f
            ListStatus.WATCHING, ListStatus.REPEATING -> 0.85f
            ListStatus.PAUSED -> 0.45f
            ListStatus.PLANNING -> 0.35f
            ListStatus.DROPPED -> -0.4f
            null -> null
        }?.let { sum += it; parts++ }
        if (score != null && score > 0) {
            sum += ((score / 10f - 5.5f) / 4.5f).coerceIn(-1f, 1f)
            parts++
        }
        if (parts == 0) return if (completed) 0.6f else 0.4f
        var a = sum / parts
        if (completed) a = max(a, 0.6f)
        return a.coerceIn(-1f, 1f)
    }

    /** Stable key of a title (AniList id, else MAL id). */
    fun keyOf(ref: MediaRef): String? = ref.anilistId?.toString() ?: ref.malId?.let { "mal:$it" }

    fun decadeOf(year: Int?): Int? = year?.takeIf { it > 1900 }?.let { it / 10 * 10 }

    private fun normalize(m: Map<String, Float>): Map<String, Float> {
        val top = m.values.maxOrNull() ?: return emptyMap()
        if (top <= 0f) return emptyMap()
        return m.mapValues { it.value / top }.filterValues { it > 0f }
    }

    private fun <K> addAll(into: MutableMap<K, Float>, keys: Iterable<K>, w: Float) {
        for (k in keys) into[k] = (into[k] ?: 0f) + w
    }

    /** Classic `buildTasteProfile`: only positive signals shape taste; null without any positive signal. */
    fun profile(signals: List<Signal>): TasteProfile? {
        val genres = mutableMapOf<String, Float>()
        val studios = mutableMapOf<String, Float>()
        val eras = mutableMapOf<Int, Float>()
        val scores = mutableListOf<Int>()
        var sample = 0
        for (s in signals) {
            s.userScore?.takeIf { it > 0 }?.let { scores += it }
            if (s.affinity <= 0f) continue
            sample++
            addAll(genres, s.media.genres, s.affinity)
            addAll(studios, s.studios, s.affinity)
            decadeOf(s.media.year)?.let { addAll(eras, listOf(it), s.affinity) }
        }
        if (sample == 0) return null
        val preferred = if (scores.isNotEmpty()) scores.average().toInt() else {
            signals.filter { it.affinity > 0f }.mapNotNull { it.media.score }.takeIf { it.isNotEmpty() }?.average()?.toInt()
        }
        val topEra = eras.values.maxOrNull() ?: 0f
        return TasteProfile(
            genres = normalize(genres),
            studios = normalize(studios),
            eras = if (topEra > 0f) eras.mapValues { it.value / topEra } else emptyMap(),
            preferredScore = preferred,
            sampleSize = sample,
        )
    }

    fun cosine(a: Map<String, Float>, b: Map<String, Float>): Float {
        var dot = 0f
        var na = 0f
        var nb = 0f
        for ((k, v) in a) {
            na += v * v
            b[k]?.let { dot += v * it }
        }
        for (v in b.values) nb += v * v
        if (na == 0f || nb == 0f) return 0f
        return dot / (sqrt(na) * sqrt(nb))
    }

    /**
     * Classic `scoreCandidate` content blend (genre 0.45, rating fit 0.2, studio 0.15, era 0.1, similar-to bonus 0.1), 0..1. Reasons are
     * the factors that pass a threshold, strongest first, so every item explains itself (checklist home.recs.explanations).
     */
    fun score(c: Candidate, profile: TasteProfile, currentYear: Int): RecommendationItem {
        val genreVec = c.media.genres.associateWith { 1f }
        val genreMatch = cosine(genreVec, profile.genres)
        val studioMatch = c.studios.maxOfOrNull { profile.studios[it] ?: 0f } ?: 0f
        val era = decadeOf(c.media.year)
        val eraMatch = era?.let { profile.eras[it] } ?: 0f
        val ratingMatch = run {
            val cand = c.media.score ?: return@run 0f
            val pref = profile.preferredScore ?: 70
            val diff = (cand - pref) / 10f
            exp(-(diff * diff) / (2 * 1.5f * 1.5f))
        }
        val freshness = c.media.year?.let { (1f - (currentYear - it) / 25f).coerceIn(0f, 1f) } ?: 0f
        val similar = if (c.similarTo != null) 1f else 0f
        val raw = genreMatch * 0.45f + ratingMatch * 0.2f + studioMatch * 0.15f + (eraMatch * 0.7f + freshness * 0.3f) * 0.1f + similar * 0.1f
        val reasons = buildList {
            if (c.similarTo != null) add(similar to RecReason(RecReasonKind.SIMILAR_TO, c.similarTo))
            val bestGenre = c.media.genres.maxByOrNull { profile.genres[it] ?: 0f }
            if (bestGenre != null && (profile.genres[bestGenre] ?: 0f) >= 0.35f) add(genreMatch to RecReason(RecReasonKind.GENRE, bestGenre))
            val bestStudio = c.studios.maxByOrNull { profile.studios[it] ?: 0f }
            if (bestStudio != null && studioMatch >= 0.3f) add(studioMatch to RecReason(RecReasonKind.STUDIO, bestStudio))
            if (era != null && eraMatch >= 0.5f) add(eraMatch * 0.6f to RecReason(RecReasonKind.ERA, "${era}s"))
            if (ratingMatch >= 0.8f && c.media.score != null) add(ratingMatch * 0.5f to RecReason(RecReasonKind.SCORE, c.media.score.toString()))
        }.sortedByDescending { it.first }.map { it.second }.take(3)
        return RecommendationItem(media = c.media, affinity = raw.coerceIn(0f, 1f), reasons = reasons)
    }

    /** Scores, drops seen / dismissed titles and duplicates, best first. */
    fun rank(
        candidates: List<Candidate>,
        profile: TasteProfile,
        seen: Set<String>,
        dismissed: Set<String>,
        limit: Int,
        currentYear: Int,
    ): List<RecommendationItem> {
        val merged = LinkedHashMap<String, Candidate>()
        for (c in candidates) {
            val key = keyOf(c.media.ref) ?: continue
            if (key in seen || key in dismissed) continue
            val prev = merged[key]
            merged[key] = if (prev == null) c else prev.copy(
                studios = prev.studios.ifEmpty { c.studios },
                similarTo = prev.similarTo ?: c.similarTo,
            )
        }
        return merged.values.map { score(it, profile, currentYear) }
            .sortedWith(compareByDescending<RecommendationItem> { it.affinity }.thenBy { it.media.title })
            .take(limit.coerceAtLeast(0))
    }

    // ---- atlas (classic atlasEmbedding: genre vectors -> 2D projection -> clusters) ----

    /**
     * Deterministic 2D map: one genre vector per title, centred, projected on the two principal axes (power iteration from fixed seeds),
     * scaled to 0..1, then k-means ([clusters], farthest-point seeding from the first title). Same input = same output.
     */
    fun atlas(seen: List<Media>, recommended: List<Media>, clusters: Int = 5): List<AtlasPoint> {
        val all = (seen.map { it to false } + recommended.map { it to true }).distinctBy { keyOf(it.first.ref) ?: it.first.title }
        if (all.isEmpty()) return emptyList()
        val vocab = all.flatMap { it.first.genres }.distinct().sorted()
        if (vocab.isEmpty()) return all.mapIndexed { i, (m, rec) -> AtlasPoint(m, (i % 10) / 9f, (i / 10 % 10) / 9f, 0, rec) }
        val dim = vocab.size
        val index = vocab.withIndex().associate { it.value to it.index }
        val vectors = all.map { (m, _) ->
            val v = FloatArray(dim)
            val n = m.genres.size.coerceAtLeast(1)
            m.genres.forEach { g -> index[g]?.let { v[it] = 1f / sqrt(n.toFloat()) } }
            v
        }
        val mean = FloatArray(dim) { d -> vectors.map { it[d] }.average().toFloat() }
        val centred = vectors.map { v -> FloatArray(dim) { v[it] - mean[it] } }
        val pc1 = principal(centred, dim, seed = 1, orthogonalTo = null)
        val pc2 = principal(centred, dim, seed = 2, orthogonalTo = pc1)
        val xs = centred.map { dot(it, pc1) }
        val ys = centred.map { dot(it, pc2) }
        fun scale(values: List<Float>): List<Float> {
            val lo = values.min()
            val hi = values.max()
            return if (hi - lo < 1e-6f) values.map { 0.5f } else values.map { (it - lo) / (hi - lo) }
        }
        val sx = scale(xs)
        val sy = scale(ys)
        val labels = kMeans(sx.zip(sy), clusters.coerceIn(1, all.size))
        return all.mapIndexed { i, (m, rec) -> AtlasPoint(m, sx[i], sy[i], labels[i], rec) }
    }

    private fun dot(a: FloatArray, b: FloatArray): Float {
        var s = 0f
        for (i in a.indices) s += a[i] * b[i]
        return s
    }

    private fun principal(rows: List<FloatArray>, dim: Int, seed: Int, orthogonalTo: FloatArray?): FloatArray {
        var v = FloatArray(dim) { i -> 1f + ((i * 31 + seed * 17) % 7) / 7f }
        repeat(40) {
            val next = FloatArray(dim)
            for (r in rows) {
                val p = dot(r, v)
                for (i in 0 until dim) next[i] += r[i] * p
            }
            if (orthogonalTo != null) {
                val proj = dot(next, orthogonalTo)
                for (i in 0 until dim) next[i] -= proj * orthogonalTo[i]
            }
            val norm = sqrt(dot(next, next))
            if (norm < 1e-9f) {
                // Degenerate (all rows equal along this axis): fall back to a fixed axis.
                return FloatArray(dim) { i -> if (i == (seed - 1).coerceAtMost(dim - 1)) 1f else 0f }
            }
            v = FloatArray(dim) { next[it] / norm }
        }
        return v
    }

    private fun kMeans(points: List<Pair<Float, Float>>, k: Int): List<Int> {
        if (k <= 1) return List(points.size) { 0 }
        val centres = mutableListOf(points[0])
        while (centres.size < k) {
            val far = points.maxByOrNull { p -> centres.minOf { c -> dist(p, c) } } ?: break
            if (centres.any { dist(it, far) < 1e-9f }) break
            centres += far
        }
        var labels = List(points.size) { 0 }
        repeat(20) {
            labels = points.map { p -> centres.indices.minByOrNull { dist(p, centres[it]) } ?: 0 }
            for (c in centres.indices) {
                val members = points.filterIndexed { i, _ -> labels[i] == c }
                if (members.isNotEmpty()) centres[c] = members.map { it.first }.average().toFloat() to members.map { it.second }.average().toFloat()
            }
        }
        return labels
    }

    private fun dist(a: Pair<Float, Float>, b: Pair<Float, Float>): Float = abs(a.first - b.first) + abs(a.second - b.second)
}
