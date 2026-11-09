// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.library

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.Media

// Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.3). Owner: worker q. Signatures FINAL (Details mounts
// ListButton; LibraryScreen mounts the tabs). Bodies: worker q, reading ListsRepository through LibraryViewModel / a small ViewModel of its own.

/**
 * Library › Listas: status segmented (every [app.tatakai.next.model.ListStatus]) + grid of `ListEntry` cards (poster, title,
 * "progress / total", score), tap -> Details. Without a linked account the list still works locally and the empty state offers
 * [onOpenAccounts] ("Vincular AniList o MAL"). Three states (loading / empty / error with retry).
 */
@Composable
fun ListsTab(
    onOpenDetails: (anilistId: Int) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("ListsTab", modifier)
}

/** Library › Favoritos: `ListsRepository.favorites()` as a poster grid; long-press / overflow removes the favorite. */
@Composable
fun FavoritesTab(
    onOpenDetails: (anilistId: Int) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("FavoritesTab", modifier)
}

/**
 * "Añadir a lista" button of Details (classic `WatchlistButton`), mounted by worker o in the Details header / floating toolbar.
 * Split button: label = current status or "Añadir a lista"; the menu lists every status + "Quitar de la lista" and a favorite toggle
 * (heart). Writes through `ListsRepository.setStatus` / `setFavorite`; offline writes are queued (snackbar "Se sincronizará").
 * [compact] = icon-only variant for the floating toolbar.
 */
@Composable
fun ListButton(
    media: Media,
    modifier: Modifier = Modifier,
    compact: Boolean = false,
) {
    Text("ListButton", modifier)
}
