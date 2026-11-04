// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.release

import app.tatakai.next.model.INITIAL_RELIABILITY
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.model.normalizeLangCodes

// Port 1:1 of apps/desktop-native/crates/tk-core/src/sources.rs::{score, rank, update_reliability, lang_codes} (docs/redesign/12 §3.1).
// Decision (12 §7): the ranking lives here in :core:release (pure JVM, same owner as the release parser) and not in :core:extensions.
// Oracle: contracts/fixtures/sources/sources-for.json + the order asserted by tk-core/tests/sources_ranking.rs. Bodies: worker a.

const val RELIABILITY_ALPHA: Float = 0.2f

/** Score bonus of a SeaDex "best" torrent release. */
const val SEADEX_BEST_BONUS: Float = 4f

/**
 * `reliability*50 ± 20 (preferred kind) + 6*min(rank,3) − 25 if below minQuality + 10 (subLang in subs) + 6 (dubLang in dubs)
 * + torrent: min(seeders,500)/50 − 20 if seeders < 20 + SEADEX_BEST_BONUS if tags.seadexBest | web: −min(latency,1000)/100
 * − 1000 when the extension is disabled`. Desktop has NO batch bonus (score does not get the episode); neither does this port.
 */
fun score(c: SourceCandidate, prefs: SourcePrefs, extEnabled: Boolean): Float {
    var v = c.reliability * 50f
    val kind = prefs.prefer
    if (kind != null) v += if (c.kind == kind) 20f else -20f
    val q = c.quality.rank()
    v += minOf(q, 3).toFloat() * 6f
    if (q < prefs.minQuality.rank()) v -= 25f
    if (c.subs.any { it == prefs.subLang }) v += 10f
    if (c.dubs.any { it == prefs.dubLang }) v += 6f
    when (c.kind) {
        SourceKind.TORRENT -> {
            val seeders = c.torrent?.seeders ?: 0
            v += minOf(seeders, 500).toFloat() / 50f
            if (seeders < 20) v -= 20f
            if (c.tags.seadexBest) v += SEADEX_BEST_BONUS
        }
        SourceKind.WEB -> {
            // not measured yet = no penalty
            val ms = c.web?.latencyMs
            if (ms != null) v -= minOf(ms, 1000).toFloat() / 100f
        }
    }
    if (!extEnabled) v -= 1000f
    return v
}

/** Score every candidate and sort by score, highest first; stable (ties keep the input order). [extEnabled] gets `extId`. */
fun rank(candidates: List<SourceCandidate>, prefs: SourcePrefs, extEnabled: (String) -> Boolean): List<Pair<SourceCandidate, Float>> =
    // sortedByDescending is stable: equal scores keep the input order (Rust sort_by + total_cmp)
    candidates.map { it to score(it, prefs, extEnabled(it.extId)) }.sortedByDescending { it.second }

/** `prev*(1-α) + α*(ok ? 1 : 0)`, α = [RELIABILITY_ALPHA], clamped 0..1; non-finite prev -> INITIAL_RELIABILITY. */
fun updateReliability(prev: Float, ok: Boolean): Float {
    val p = if (prev.isFinite()) prev.coerceIn(0f, 1f) else INITIAL_RELIABILITY
    val hit = if (ok) 1f else 0f
    return (p * (1f - RELIABILITY_ALPHA) + RELIABILITY_ALPHA * hit).coerceIn(0f, 1f)
}

/** Lowercase ISO 639-1 (`es-419` -> `es`), dropping `und` / empty / duplicates, first-seen order. */
fun langCodes(tags: Iterable<String>): List<String> = normalizeLangCodes(tags)
