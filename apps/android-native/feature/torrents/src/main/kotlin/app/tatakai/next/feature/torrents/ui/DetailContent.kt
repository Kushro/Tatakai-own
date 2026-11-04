// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.Bookmark
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.Pause
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.feature.torrents.R
import app.tatakai.next.feature.torrents.vm.TorrentDetailViewModel
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.torrent.TorrentSection
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.labels.labelRes
import kotlinx.coroutines.launch

/** The four per-torrent sections in tab order. */
internal val DETAIL_SECTIONS = listOf(TorrentSection.OVERVIEW, TorrentSection.FILES, TorrentSection.PEERS, TorrentSection.TRACKERS)

/**
 * Detail of one torrent (11 §3.3): fixed header (name, state pill, pause / resume, menu) + PrimaryScrollableTabRow + HorizontalPager
 * with Overview / Files / Peers / Trackers. [showBack] false when embedded next to the library (expanded list-detail).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun DetailContent(
    viewModel: TorrentDetailViewModel,
    initialSection: TorrentSection,
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
    showBack: Boolean = true,
) {
    val stats by viewModel.stats.collectAsStateWithLifecycle()
    val item by viewModel.item.collectAsStateWithLifecycle()
    val removed by viewModel.removed.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    MessagesEffect(viewModel.messages, snackbar)
    LaunchedEffect(removed) { if (removed) onBack() }

    val loaded = (stats as? LoadState.Ok)?.value
    val title = loaded?.name ?: item?.series ?: item?.name ?: ""
    var confirmRemove by remember { mutableStateOf(false) }
    val context = LocalContext.current

    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(UiR.string.nav_torrents)) },
                navigationIcon = {
                    if (showBack) {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
                        }
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            // fixed header
            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    loaded?.let { StatePill(it.state) }
                    Spacer(Modifier.weight(1f))
                    if (loaded != null) {
                        if (loaded.state == TorrentState.PAUSED) {
                            IconButton(onClick = viewModel::resume) {
                                Icon(Icons.Outlined.PlayArrow, contentDescription = stringResource(R.string.torrent_overview_resume))
                            }
                        } else {
                            IconButton(onClick = viewModel::pause) {
                                Icon(Icons.Outlined.Pause, contentDescription = stringResource(R.string.torrent_overview_pause))
                            }
                        }
                    }
                    var menu by remember { mutableStateOf(false) }
                    Box {
                        IconButton(onClick = { menu = true }) {
                            Icon(Icons.Outlined.MoreVert, contentDescription = stringResource(R.string.torrent_library_actions))
                        }
                        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                            val copyLabel = stringResource(R.string.torrent_overview_copy_hash)
                            val copied = stringResource(R.string.torrent_android_hash_copied)
                            DropdownMenuItem(
                                text = { Text(copyLabel) },
                                leadingIcon = { Icon(Icons.Outlined.ContentCopy, null) },
                                onClick = {
                                    menu = false
                                    copyToClipboard(context, copyLabel, viewModel.torrentId)
                                    snackbar.show(copied)
                                },
                            )
                            val shareTitle = stringResource(R.string.torrent_android_share_magnet)
                            DropdownMenuItem(
                                text = { Text(shareTitle) },
                                leadingIcon = { Icon(Icons.Outlined.Share, null) },
                                onClick = {
                                    menu = false
                                    shareText(context, magnetOf(viewModel.torrentId), shareTitle)
                                },
                            )
                            item?.let { current ->
                                DropdownMenuItem(
                                    text = { Text(stringResource(if (current.persist) R.string.torrent_library_unkeep else R.string.torrent_library_keep)) },
                                    leadingIcon = { Icon(if (current.persist) Icons.Outlined.Bookmark else Icons.Outlined.BookmarkBorder, null) },
                                    onClick = {
                                        menu = false
                                        viewModel.setPersist(!current.persist)
                                    },
                                )
                            }
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.torrent_overview_remove)) },
                                leadingIcon = { Icon(Icons.Outlined.Delete, null) },
                                onClick = {
                                    menu = false
                                    confirmRemove = true
                                },
                            )
                        }
                    }
                }
            }

            val initialPage = DETAIL_SECTIONS.indexOf(initialSection).coerceAtLeast(0)
            val pager = rememberPagerState(initialPage = initialPage) { DETAIL_SECTIONS.size }
            val scope = rememberCoroutineScope()
            LaunchedEffect(initialPage) { if (pager.currentPage != initialPage) pager.scrollToPage(initialPage) }
            PrimaryScrollableTabRow(selectedTabIndex = pager.currentPage) {
                DETAIL_SECTIONS.forEachIndexed { index, section ->
                    Tab(
                        selected = pager.currentPage == index,
                        onClick = { scope.launch { pager.animateScrollToPage(index) } },
                        text = { Text(stringResource(section.labelRes())) },
                    )
                }
            }
            HorizontalPager(state = pager, modifier = Modifier.fillMaxSize()) { page ->
                when (DETAIL_SECTIONS[page]) {
                    TorrentSection.OVERVIEW -> OverviewTab(viewModel, item)
                    TorrentSection.FILES -> FilesTab(viewModel, item, onPlay)
                    TorrentSection.PEERS -> PeersTab(viewModel)
                    else -> TrackersTab(viewModel)
                }
            }
        }
    }

    if (confirmRemove) {
        DeleteDialog(
            count = 1,
            onConfirm = { deleteFiles ->
                confirmRemove = false
                viewModel.remove(deleteFiles)
            },
            onDismiss = { confirmRemove = false },
        )
    }
}
