// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.DownloadsTab
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.state.WaveBPlaceholder
import org.koin.compose.viewmodel.koinViewModel

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r; checklist downloads.page / .history /
 * .local-library.* / .integrity-check / .storage-limit): `Route.Downloads(tab)`. Tabs = `DownloadsTab` (Activas · Anime · Manga · Novelas ·
 * Historial · Biblioteca local); search field; rows with progress, ETA, pause / resume / cancel / retry; torrents rows open the client
 * ([onOpenTorrent]); completed episodes play offline ([onPlay] with `PlayerArgs.localUri`); chapters open their reader offline.
 * [onBack] null on wide windows (rail destination). Signature FINAL (the shell wires it); body: r.
 */
@Composable
fun DownloadsScreen(
    initialTab: DownloadsTab,
    onBack: (() -> Unit)?,
    onPlay: (PlayerArgs) -> Unit,
    onOpenDetails: (anilistId: Int) -> Unit,
    onOpenTorrent: (torrentId: String) -> Unit,
    onOpenMangaReader: (MangaReaderArgs) -> Unit,
    onOpenNovelReader: (NovelReaderArgs) -> Unit,
    modifier: Modifier = Modifier,
    viewModel: DownloadsViewModel = koinViewModel(),
) {
    WaveBPlaceholder(title = stringResource(R.string.downloads_title), modifier = modifier, onBack = onBack)
}
