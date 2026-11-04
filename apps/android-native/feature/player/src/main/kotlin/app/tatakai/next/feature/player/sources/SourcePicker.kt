// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player.sources

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.AppError
import app.tatakai.next.model.RankedSource
import app.tatakai.next.model.SourceMode

// android wave contract (docs/redesign/12 §3.11). Owner: worker d; also called by worker c (Details). Names and signatures FIXED.

/**
 * Source selector (07 §3.4, 11 §3.6): `ModalBottomSheet` on compact, side sheet (AdaptiveSheet) elsewhere. Banner with the current
 * [mode] + link to Settings ([onOpenSettings]), segmented All / Web / Torrent, horizontally scrolling FilterChips (BD/WEB/REMUX,
 * resolution, codec, 10-bit, Dual audio, Multi-sub, Batch, SeaDex best, min reliability, "Compatible with this device"), list in RANKED
 * order (filtering never reorders), filter state local (`rememberSaveable`). Loading / empty / error states inside.
 * [onDownload] non-null adds the secondary "Descargar" action to torrent rows (Details passes it; the player passes null).
 */
@Composable
fun SourcePickerSheet(
    sources: List<RankedSource>,
    loading: Boolean,
    error: AppError?,
    mode: SourceMode,
    onPick: (RankedSource) -> Unit,
    onDismiss: () -> Unit,
    onRetry: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
    onDownload: ((RankedSource) -> Unit)? = null,
) {
    Text("SourcePickerSheet", modifier)
}

/**
 * One source row (07 §3.4 `SourceRow`): kind icon, `Ext · Server/Group`, pills Webstream/Torrent, Recomendada, Ext desactivada, tags as
 * small AssistChips (SeaDex best = primaryContainer), key-value facts (web: server, quality+container, latency, subs, dubs; torrent:
 * group, quality+codec, size, seeders, leechers, subs, dubs, hash), reliability bar (PillTone OK/WARN/ERR). Used by the picker and by
 * the Details source summary panel (worker c).
 */
@Composable
fun SourceRow(
    source: RankedSource,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    compact: Boolean = false,
    onDownload: (() -> Unit)? = null,
) {
    Text("SourceRow", modifier)
}
