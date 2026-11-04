// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.HelpOutline
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.PeopleOutline
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.Podcasts
import androidx.compose.material.icons.outlined.SaveAlt
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Stream
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.WavyProgress
import app.tatakai.next.feature.torrents.R
import app.tatakai.next.feature.torrents.vm.PeerSort
import app.tatakai.next.feature.torrents.vm.TorrentDetailViewModel
import app.tatakai.next.feature.torrents.vm.filterFiles
import app.tatakai.next.feature.torrents.vm.sortPeers
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.torrent.TorrentFile
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentPeer
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.model.torrent.TorrentStats
import app.tatakai.next.model.torrent.TorrentTracker
import app.tatakai.next.model.torrent.TrackerStatus
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.formatBytes
import app.tatakai.next.ui.format.formatRate
import app.tatakai.next.ui.labels.labelRes

// The four per-torrent tabs of 11 §3.3 (desktop C screens of 10 §5.4). Each one: loading / empty / error.

// ---------------------------------------------------------------------------------------------------------------------------------
// Overview
// ---------------------------------------------------------------------------------------------------------------------------------

@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun OverviewTab(viewModel: TorrentDetailViewModel, item: TorrentLibraryItem?) {
    val state by viewModel.stats.collectAsStateWithLifecycle()
    TorrentLoadState(state, viewModel::retryStats, Modifier.fillMaxSize(), errorTitle = stringResource(R.string.torrent_overview_error)) { s ->
        OverviewBody(s, item, onPersist = viewModel::setPersist, onSeed = viewModel::setSeeding)
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun OverviewBody(s: TorrentStats, item: TorrentLibraryItem?, onPersist: (Boolean) -> Unit, onSeed: (Boolean) -> Unit) {
    val context = LocalContext.current
    val snackbar = LocalAppSnackbar.current
    Column(
        Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp)
            .navigationBarsPadding(),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        if (s.state == TorrentState.ERROR) {
            Banner(s.error ?: stringResource(R.string.torrent_overview_error), error = true)
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Box(contentAlignment = Alignment.Center) {
                WavyProgress(progress = s.progress.toFloat(), modifier = Modifier.size(96.dp), circular = true)
                Text(percentText(s.progress), style = monoStyle(MaterialTheme.typography.titleMedium))
            }
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(stringResource(R.string.torrent_overview_progress), style = MaterialTheme.typography.labelLarge)
                Text(
                    stringResource(R.string.torrent_overview_size_of, formatBytes(s.downloadedBytes), formatBytes(s.totalBytes)),
                    style = monoStyle(),
                )
                Text(
                    when {
                        s.streaming -> stringResource(R.string.torrent_overview_streaming)
                        s.progress >= 1.0 -> stringResource(R.string.torrent_overview_complete)
                        else -> stringResource(R.string.torrent_overview_remaining_value, durationText(s.etaSecs))
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        SectionTitle(stringResource(R.string.torrent_overview_transfer))
        FigureGrid(
            listOf(
                stringResource(R.string.torrent_overview_downloaded) to formatBytes(s.downloadedBytes),
                stringResource(R.string.torrent_overview_uploaded) to formatBytes(s.uploadedBytes),
                stringResource(R.string.torrent_overview_size) to stringResource(R.string.torrent_overview_size_of, formatBytes(s.totalBytes), formatBytes(s.sizeBytes)),
                stringResource(R.string.torrent_overview_pieces) to stringResource(R.string.torrent_overview_pieces_value, s.piecesHave, s.pieces, formatBytes(s.pieceBytes)),
                stringResource(R.string.torrent_overview_down_speed) to formatRate(s.downBps),
                stringResource(R.string.torrent_overview_up_speed) to formatRate(s.upBps),
                stringResource(R.string.torrent_overview_remaining) to (if (s.etaSecs == null) stringResource(R.string.torrent_overview_eta_unknown) else durationText(s.etaSecs)),
                stringResource(R.string.torrent_overview_elapsed) to durationText(s.elapsedSecs),
            ),
        )

        SectionTitle(stringResource(R.string.torrent_overview_swarm))
        val ratio = if (s.downloadedBytes > 0) s.uploadedBytes.toDouble() / s.downloadedBytes else 0.0
        FigureGrid(
            listOf(
                stringResource(R.string.torrent_overview_seeders) to s.seeders.toString(),
                stringResource(R.string.torrent_overview_leechers) to s.leechers.toString(),
                stringResource(R.string.torrent_overview_wires) to s.peers.toString(),
                stringResource(R.string.torrent_overview_ratio) to stringResource(R.string.torrent_fmt_ratio, ratio),
            ),
        )

        SectionTitle(stringResource(R.string.torrent_overview_info_hash))
        val copyLabel = stringResource(R.string.torrent_overview_copy_hash)
        val copied = stringResource(R.string.torrent_android_hash_copied)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(s.infoHash, style = monoStyle(MaterialTheme.typography.bodySmall), modifier = Modifier.weight(1f))
            IconButton(onClick = {
                copyToClipboard(context, copyLabel, s.infoHash)
                snackbar.show(copied)
            }) {
                Icon(Icons.Outlined.ContentCopy, contentDescription = copyLabel)
            }
        }

        SectionTitle(stringResource(R.string.torrent_overview_protocol))
        val on = stringResource(R.string.torrent_overview_proto_on)
        val off = stringResource(R.string.torrent_overview_proto_off)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            listOf(
                UiR.string.torrent_proto_dht to s.protocol.dht,
                UiR.string.torrent_proto_lsd to s.protocol.lsd,
                UiR.string.torrent_proto_pex to s.protocol.pex,
                UiR.string.torrent_proto_nat to s.protocol.nat,
                UiR.string.torrent_proto_forwarding to s.protocol.forwarding,
                UiR.string.torrent_proto_persisting to s.protocol.persisting,
                UiR.string.torrent_proto_streaming to s.protocol.streaming,
            ).forEach { (label, active) ->
                val text = stringResource(label)
                Pill(
                    text = text,
                    tone = if (active) PillTone.OK else PillTone.NEUTRAL,
                    modifier = Modifier.semanticsLabel("$text: ${if (active) on else off}"),
                )
            }
        }

        if (item != null) {
            HorizontalDivider()
            SwitchRow(
                title = stringResource(R.string.torrent_library_keep),
                hint = stringResource(if (item.persist) R.string.torrent_library_kept else R.string.torrent_library_not_kept),
                checked = item.persist,
                onChange = onPersist,
            )
            SwitchRow(
                title = stringResource(R.string.torrent_android_seed),
                hint = stringResource(R.string.torrent_android_seed_hint),
                checked = item.seed,
                onChange = onSeed,
            )
        }
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary)
}

/** 2-column grid of label / value cells. */
@Composable
private fun FigureGrid(cells: List<Pair<String, String>>) {
    Column {
        cells.chunked(2).forEach { row ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                row.forEach { (label, value) -> FigureCell(label, value, Modifier.weight(1f)) }
                if (row.size == 1) Box(Modifier.weight(1f))
            }
        }
    }
}

@Composable
internal fun SwitchRow(title: String, hint: String?, checked: Boolean, onChange: (Boolean) -> Unit, enabled: Boolean = true) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = if (hint != null) {
            { Text(hint) }
        } else {
            null
        },
        trailingContent = { Switch(checked = checked, onCheckedChange = onChange, enabled = enabled) },
        modifier = Modifier.clickable(enabled = enabled) { onChange(!checked) },
    )
}

private fun Modifier.semanticsLabel(label: String): Modifier = this.semantics { contentDescription = label }

// ---------------------------------------------------------------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------------------------------------------------------------

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun FilesTab(viewModel: TorrentDetailViewModel, item: TorrentLibraryItem?, onPlay: (PlayerArgs) -> Unit) {
    val state by viewModel.files.collectAsStateWithLifecycle()
    var query by rememberSaveable { mutableStateOf("") }
    var sheetFor by remember { mutableStateOf<TorrentFile?>(null) }
    TorrentLoadState(state, viewModel::retryFiles, Modifier.fillMaxSize(), errorTitle = stringResource(R.string.torrent_files_error)) { files ->
        if (files.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.FolderOpen,
                title = stringResource(R.string.torrent_files_empty_title),
                message = stringResource(R.string.torrent_files_empty_body),
            )
            return@TorrentLoadState
        }
        val shown = filterFiles(files, query)
        Column(Modifier.fillMaxSize()) {
            OutlinedTextField(
                value = query,
                onValueChange = { query = it },
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
                singleLine = true,
                label = { Text(stringResource(R.string.torrent_files_search)) },
                placeholder = { Text(stringResource(R.string.torrent_files_search_placeholder)) },
                leadingIcon = { Icon(Icons.Outlined.Search, null) },
                trailingIcon = {
                    if (query.isNotEmpty()) {
                        IconButton(onClick = { query = "" }) {
                            Icon(Icons.Outlined.Close, contentDescription = stringResource(R.string.torrent_files_clear_search))
                        }
                    }
                },
            )
            val selected = files.count { it.selected }
            Text(
                stringResource(
                    R.string.torrent_files_summary,
                    pluralStringResource(R.plurals.torrent_files_count, files.size, files.size),
                    pluralStringResource(R.plurals.torrent_files_selected_n, selected, selected),
                    formatBytes(files.filter { it.selected }.sumOf { it.sizeBytes }),
                ),
                style = monoStyle(MaterialTheme.typography.labelMedium),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
            if (shown.isEmpty()) {
                EmptyState(
                    icon = Icons.Outlined.Search,
                    title = stringResource(R.string.torrent_files_no_match),
                    message = stringResource(R.string.torrent_files_no_match_body, query),
                    actionLabel = stringResource(R.string.torrent_files_clear_search),
                    onAction = { query = "" },
                )
            } else {
                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                    items(shown, key = { it.index }, contentType = { "file" }) { file -> FileRow(file) { sheetFor = file } }
                }
            }
        }
    }
    sheetFor?.let { file ->
        ModalBottomSheet(onDismissRequest = { sheetFor = null }) {
            Column(Modifier.padding(bottom = 24.dp)) {
                Text(
                    file.name,
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
                ListItem(
                    headlineContent = { Text(stringResource(R.string.torrent_files_play)) },
                    leadingContent = { Icon(Icons.Outlined.PlayArrow, null) },
                    modifier = Modifier.clickable {
                        sheetFor = null
                        onPlay(playArgsFor(item, viewModel.torrentId, file.index, file.episode, file.name))
                    },
                )
                if (!file.selected) {
                    ListItem(
                        headlineContent = { Text(stringResource(R.string.torrent_files_download_only)) },
                        leadingContent = { Icon(Icons.Outlined.Download, null) },
                        modifier = Modifier.clickable {
                            sheetFor = null
                            viewModel.downloadFile(file.index)
                        },
                    )
                }
                if (file.progress >= 1.0) {
                    ListItem(
                        headlineContent = { Text(stringResource(R.string.torrent_android_save_downloads)) },
                        leadingContent = { Icon(Icons.Outlined.SaveAlt, null) },
                        modifier = Modifier.clickable {
                            sheetFor = null
                            viewModel.export(file.index)
                        },
                    )
                }
            }
        }
    }
}

@Composable
private fun FileRow(file: TorrentFile, onClick: () -> Unit) {
    ListItem(
        modifier = Modifier.clickable(onClick = onClick),
        headlineContent = { Text(file.name, maxLines = 2, overflow = TextOverflow.Ellipsis) },
        supportingContent = {
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                    file.episode?.let { Pill(stringResource(R.string.torrent_files_episode_value, it), tone = PillTone.NEUTRAL) }
                    Text(
                        stringResource(R.string.torrent_files_size_progress, formatBytes(file.sizeBytes), percentText(file.progress)),
                        style = monoStyle(MaterialTheme.typography.bodySmall),
                    )
                    if (!file.selected) {
                        Text(stringResource(R.string.torrent_files_skipped), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    }
                }
                WavyProgress(progress = file.progress.toFloat(), modifier = Modifier.fillMaxWidth(), thin = true)
            }
        },
        trailingContent = if (file.streams > 0) {
            { Icon(Icons.Outlined.Stream, contentDescription = stringResource(R.string.torrent_files_streaming), tint = MaterialTheme.colorScheme.primary) }
        } else {
            null
        },
    )
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Peers
// ---------------------------------------------------------------------------------------------------------------------------------

private val FLAG_LETTERS = listOf("d", "u", "i", "p", "e", "x", "h", "s")

@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun PeersTab(viewModel: TorrentDetailViewModel) {
    val state by viewModel.peers.collectAsStateWithLifecycle()
    var sort by rememberSaveable { mutableStateOf(PeerSort.DOWN) }
    var legend by remember { mutableStateOf(false) }
    TorrentLoadState(state, viewModel::retryPeers, Modifier.fillMaxSize(), errorTitle = stringResource(R.string.torrent_peers_error)) { peers ->
        Column(Modifier.fillMaxSize()) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Segmented(
                    options = listOf(
                        SegmentedOption(PeerSort.DOWN, stringResource(R.string.torrent_peers_col_down)),
                        SegmentedOption(PeerSort.UP, stringResource(R.string.torrent_peers_col_up)),
                        SegmentedOption(PeerSort.PROGRESS, stringResource(R.string.torrent_peers_col_progress)),
                    ),
                    selected = sort,
                    onSelect = { sort = it },
                    modifier = Modifier.weight(1f),
                    small = true,
                )
                IconButton(onClick = { legend = true }) {
                    Icon(Icons.AutoMirrored.Outlined.HelpOutline, contentDescription = stringResource(R.string.torrent_peers_legend))
                }
            }
            if (peers.isEmpty()) {
                EmptyState(
                    icon = Icons.Outlined.PeopleOutline,
                    title = stringResource(R.string.torrent_peers_empty_title),
                    message = stringResource(R.string.torrent_peers_empty_body),
                )
            } else {
                Text(
                    stringResource(
                        R.string.torrent_peers_summary,
                        peers.size,
                        formatRate(peers.sumOf { it.downBps }),
                        formatRate(peers.sumOf { it.upBps }),
                    ),
                    style = monoStyle(MaterialTheme.typography.labelMedium),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                    items(sortPeers(peers, sort), key = { it.addr }, contentType = { "peer" }) { PeerRow(it) }
                }
            }
        }
    }
    if (legend) {
        ModalBottomSheet(onDismissRequest = { legend = false }) {
            Column(Modifier.padding(start = 24.dp, end = 24.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.torrent_peers_legend), style = MaterialTheme.typography.titleMedium)
                FLAG_LETTERS.forEach { letter ->
                    Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        Text(letter.uppercase(), style = monoStyle(MaterialTheme.typography.titleSmall))
                        Text(stringResource(flagLabel(letter)), style = MaterialTheme.typography.bodyMedium)
                    }
                }
                TextButton(onClick = { legend = false }, modifier = Modifier.align(Alignment.End)) {
                    Text(stringResource(UiR.string.ui_close))
                }
            }
        }
    }
}

private fun flagLabel(letter: String): Int = when (letter) {
    "d" -> R.string.torrent_peers_flag_d
    "u" -> R.string.torrent_peers_flag_u
    "i" -> R.string.torrent_peers_flag_i
    "p" -> R.string.torrent_peers_flag_p
    "e" -> R.string.torrent_peers_flag_e
    "x" -> R.string.torrent_peers_flag_x
    "h" -> R.string.torrent_peers_flag_h
    else -> R.string.torrent_peers_flag_s
}

@Composable
private fun PeerRow(peer: TorrentPeer) {
    ListItem(
        headlineContent = { Text(peer.addr, style = monoStyle(), maxLines = 1, overflow = TextOverflow.Ellipsis) },
        supportingContent = {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    listOfNotNull(peer.client ?: stringResource(R.string.torrent_peers_unknown), peer.country).joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    stringResource(R.string.torrent_peers_rates, formatRate(peer.downBps), formatRate(peer.upBps), percentText(peer.progress)),
                    style = monoStyle(MaterialTheme.typography.bodySmall),
                )
            }
        },
        trailingContent = if (peer.flags.isNotBlank()) {
            { Text(peer.flags, style = monoStyle(MaterialTheme.typography.labelMedium)) }
        } else {
            null
        },
        leadingContent = { Icon(Icons.Outlined.Hub, null, tint = MaterialTheme.colorScheme.onSurfaceVariant) },
    )
}

// ---------------------------------------------------------------------------------------------------------------------------------
// Trackers
// ---------------------------------------------------------------------------------------------------------------------------------

internal fun TrackerStatus.tone(): PillTone = when (this) {
    TrackerStatus.WORKING -> PillTone.OK
    TrackerStatus.FAILED -> PillTone.ERR
    TrackerStatus.UPDATING, TrackerStatus.UNKNOWN -> PillTone.NEUTRAL
}

@Composable
internal fun TrackersTab(viewModel: TorrentDetailViewModel) {
    val state by viewModel.trackers.collectAsStateWithLifecycle()
    TorrentLoadState(state, viewModel::retryTrackers, Modifier.fillMaxSize(), errorTitle = stringResource(R.string.torrent_trackers_error)) { trackers ->
        if (trackers.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.Podcasts,
                title = stringResource(R.string.torrent_trackers_empty_title),
                message = stringResource(R.string.torrent_trackers_empty_body),
            )
        } else {
            Column(Modifier.fillMaxSize()) {
                Text(
                    stringResource(R.string.torrent_trackers_summary, trackers.size, trackers.count { it.status == TrackerStatus.WORKING }),
                    style = monoStyle(MaterialTheme.typography.labelMedium),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                )
                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                    items(trackers, key = { it.url }, contentType = { "tracker" }) { TrackerRow(it) }
                }
            }
        }
    }
}

@Composable
private fun TrackerRow(tracker: TorrentTracker) {
    val none = stringResource(R.string.torrent_fmt_none)
    ListItem(
        headlineContent = { Text(tracker.url, style = monoStyle(MaterialTheme.typography.bodySmall), maxLines = 1, overflow = TextOverflow.Ellipsis) },
        supportingContent = {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    stringResource(
                        R.string.torrent_trackers_counts,
                        tracker.seeders?.toString() ?: none,
                        tracker.leechers?.toString() ?: none,
                        tracker.downloaded?.toString() ?: none,
                    ),
                    style = monoStyle(MaterialTheme.typography.bodySmall),
                )
                tracker.message?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        },
        trailingContent = { Pill(stringResource(tracker.status.labelRes()), tone = tracker.status.tone()) },
    )
}
