// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.torrent.TorrentSection

// android wave contract (docs/redesign/12 §3.12). Owner: worker f. Names and signatures FIXED (called by :app, feature/library,
// feature/settings, feature/details). Data through TorrentRepository (Koin, torrentModule). Every screen: loading / empty / error.

/**
 * Route entry of `Route.Torrents(section, torrentId)`: tablet rail destination and deep links (player "Abrir en el cliente", You ›
 * Descargas, notifications). Per-torrent sections without [torrentId] show a picker over the library or an EmptyState leading to LIBRARY.
 * Expanded: ListDetailPaneScaffold (library | detail). [onNavigate] changes section / torrent in place.
 */
@Composable
fun TorrentsScreen(
    section: TorrentSection,
    torrentId: String?,
    onNavigate: (section: TorrentSection, torrentId: String?) -> Unit,
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("TorrentsScreen", modifier)
}

/** Body of the "Torrents" tab of Library on phones (11 §3.2): cards, 3-dot menu, multi-select bar. Opening a card calls [onOpenTorrent]. */
@Composable
fun TorrentsLibraryTab(
    onOpenTorrent: (torrentId: String) -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("TorrentsLibraryTab", modifier)
}

/** Detail of one torrent: fixed header + PrimaryScrollableTabRow Overview / Files / Peers / Trackers + HorizontalPager (11 §3.3). */
@Composable
fun TorrentDetailScreen(
    torrentId: String,
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
    initialSection: TorrentSection = TorrentSection.OVERVIEW,
) {
    Text("TorrentDetailScreen", modifier)
}

/** Downloads (11 §3.4): the "Entiendo" warning screen while !backgroundAck (+ POST_NOTIFICATIONS), then switch + chips + list. */
@Composable
fun TorrentDownloadsScreen(
    onBack: () -> Unit,
    onOpenTorrent: (torrentId: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("TorrentDownloadsScreen", modifier)
}

/**
 * Settings › Torrents subpage body (11 §3.5): Velocidad, Red, Almacenamiento (SAF tree picker, free space, after-watch), Segundo plano.
 * Immediate save per field through SettingsRepository.setTorrentSettings; inline validation in supportingText. Hosted by worker b's
 * SettingsScreen (and by TorrentsScreen SETTINGS on tablets). [onBack] null = no top bar (embedded).
 */
@Composable
fun TorrentSettingsPage(
    modifier: Modifier = Modifier,
    onBack: (() -> Unit)? = null,
) {
    Text("TorrentSettingsPage", modifier)
}
