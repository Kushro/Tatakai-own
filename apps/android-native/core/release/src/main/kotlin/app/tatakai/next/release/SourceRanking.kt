// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.release

import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourcePrefs

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
fun score(c: SourceCandidate, prefs: SourcePrefs, extEnabled: Boolean): Float = TODO("worker a: port tk-core sources::score")

/** Score every candidate and sort by score, highest first; stable (ties keep the input order). [extEnabled] gets `extId`. */
fun rank(candidates: List<SourceCandidate>, prefs: SourcePrefs, extEnabled: (String) -> Boolean): List<Pair<SourceCandidate, Float>> =
    TODO("worker a: port tk-core sources::rank")

/** `prev*(1-α) + α*(ok ? 1 : 0)`, α = [RELIABILITY_ALPHA], clamped 0..1; non-finite prev -> INITIAL_RELIABILITY. */
fun updateReliability(prev: Float, ok: Boolean): Float = TODO("worker a: port tk-core sources::update_reliability")

/** Lowercase ISO 639-1 (`es-419` -> `es`), dropping `und` / empty / duplicates, first-seen order. */
fun langCodes(tags: Iterable<String>): List<String> = TODO("worker a: port tk-core sources::lang_codes")
