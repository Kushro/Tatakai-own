// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.Quality
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SourceCandidate
import app.tatakai.next.model.SourcePrefs

// parity wave A (docs/redesign/15 §3.2, bug `sources.remembered-vs-prefs`). Owner: worker p. Pure, no Android.
//
// The pick of THIS session (`PlayerArgs.sourceKey`, chosen in Details) always wins. The REMEMBERED pick (`SourceStatsStore.pickedSource`,
// from an older session) only wins while it still fits the current `SourcePrefs` and its extension is enabled; otherwise it is dropped
// (the caller clears it with `clearPickedSource` and shows a notice) and the ranking decides. Audit case: preferences JA audio + ES subs,
// remembered RU torrent pack without subtitles -> the recommended web source plays and the RU pack stays in the list as an option.

/** Why a remembered source no longer fits the preferences. */
enum class PickMismatch { EXTENSION_OFF, QUALITY, KIND, LANGUAGE }

/** What the player works on: [sources] with at most one `picked` row, and whether the remembered pick was dropped. */
data class PickResult(val sources: List<RankedSource>, val storedDiscarded: Boolean, val mismatch: PickMismatch? = null)

/** Every reason [c] (enabled = [extEnabled]) does not fit [prefs]; empty = compatible. */
fun pickMismatches(c: SourceCandidate, extEnabled: Boolean, prefs: SourcePrefs): List<PickMismatch> {
    val out = ArrayList<PickMismatch>(4)
    if (!extEnabled) out += PickMismatch.EXTENSION_OFF
    if (c.quality != Quality.UNKNOWN && c.quality.rank() < prefs.minQuality.rank()) out += PickMismatch.QUALITY
    val prefer = prefs.prefer
    if (prefer != null && c.kind != prefer) out += PickMismatch.KIND
    val subs = c.subs.mapNotNull { LanguageCodes.normalize(it) }
    val dubs = c.dubs.mapNotNull { LanguageCodes.normalize(it) }
    val wantSub = LanguageCodes.normalize(prefs.subLang)
    val wantDub = LanguageCodes.normalize(prefs.dubLang)
    // Watchable in the user's languages = subtitles in the subtitle language, or audio in the dub language. A source with no language
    // facts at all cannot prove it fits, so a remembered one of those does not override the ranking either.
    val languageOk = (wantSub != null && wantSub in subs) || (wantDub != null && wantDub in dubs) || (wantSub == null && wantDub == null)
    if (!languageOk) out += PickMismatch.LANGUAGE
    return out
}

/**
 * [requestedKey] (this session) or else [storedPick] (remembered) becomes the only `picked` row, but a remembered pick that no longer
 * fits [prefs] is ignored ([PickResult.storedDiscarded]). The order never changes. Without [prefs] it behaves like [applyPick].
 */
fun resolvePick(sources: List<RankedSource>, requestedKey: String?, storedPick: String?, prefs: SourcePrefs?): PickResult {
    val requested = requestedKey?.takeIf { k -> sources.any { it.key == k } }
    if (requested != null) return PickResult(sources.map { it.copy(picked = it.key == requested) }, storedDiscarded = false)
    val stored = storedPick?.let { k -> sources.firstOrNull { it.key == k } }
    if (stored != null) {
        val mismatch = prefs?.let { pickMismatches(stored.candidate, stored.extEnabled, it).firstOrNull() }
        if (mismatch == null) return PickResult(sources.map { it.copy(picked = it.key == stored.key) }, storedDiscarded = false)
        return PickResult(sources.map { it.copy(picked = false) }, storedDiscarded = true, mismatch = mismatch)
    }
    return PickResult(applyPick(sources, null, null), storedDiscarded = false)
}
