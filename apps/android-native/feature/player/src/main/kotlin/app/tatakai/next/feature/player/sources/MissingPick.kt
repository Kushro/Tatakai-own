// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player.sources

import app.tatakai.next.model.RankedSource

/** Longest pick name the notice shows; longer ones end in "…" so "Tu elección (…) no apareció esta vez." stays one line at 393 dp. */
internal const val MISSING_PICK_MAX_CHARS: Int = 16

/**
 * BUG-140 (parity with desktop `missing_pick_label`): the name of the user's remembered pick ([rememberedKey], a [RankedSource.key]
 * `"<extId>|<kind>|<label>"`) when this list does not have it, so the picker says so instead of showing the recommended row in silence.
 * null when nothing is remembered, the list is still arriving ([loading]: Details' `loadingMore`, the player's `candidatesLoading`;
 * BUG-142: the pick can come in a later batch, so "no apareció" is only said of the complete list), the list is empty, a row is the
 * pick or another row is already picked, or the name would only be a raw id (a 32+ hex info hash). Pure.
 */
fun missingPickLabel(sources: List<RankedSource>, rememberedKey: String?, loading: Boolean): String? {
    if (loading) return null
    val key = rememberedKey?.takeIf { it.isNotEmpty() } ?: return null
    if (sources.isEmpty() || sources.any { it.picked || it.key == key }) return null
    val label = key.split('|', limit = 3).getOrNull(2)?.trim() ?: return null
    val rawId = label.length >= 32 && label.all { it in '0'..'9' || it in 'a'..'f' || it in 'A'..'F' }
    if (label.isEmpty() || rawId) return null
    return if (label.length <= MISSING_PICK_MAX_CHARS) label else label.take(MISSING_PICK_MAX_CHARS - 1).trimEnd() + "…"
}
