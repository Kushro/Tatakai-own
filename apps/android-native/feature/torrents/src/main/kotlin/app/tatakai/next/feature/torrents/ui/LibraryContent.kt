// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents.ui

import android.text.format.DateUtils
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Bookmark
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.DownloadForOffline
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.OpenInNew
import androidx.compose.material.icons.outlined.Pause
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.SelectAll
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.WavyProgress
import app.tatakai.next.feature.torrents.R
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.ui.format.formatBytes
import app.tatakai.next.ui.media.MediaCover

/** Library list (11 §3.2): cards, 3-dot menu, long-press multi-select with a contextual bar. Stateless; the VM lives in the caller. */
@Composable
internal fun LibraryContent(
    state: LoadState<List<TorrentLibraryItem>>,
    selection: Set<String>,
    onRetry: () -> Unit,
    onOpen: (String) -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    onToggleSelect: (String) -> Unit,
    onSelectAll: (Collection<String>) -> Unit,
    onClearSelection: () -> Unit,
    onPause: (Collection<String>) -> Unit,
    onResume: (Collection<String>) -> Unit,
    onPersist: (Collection<String>, Boolean) -> Unit,
    onRemove: (Collection<String>, Boolean) -> Unit,
    modifier: Modifier = Modifier,
    selectedId: String? = null,
    header: (@Composable () -> Unit)? = null,
) {
    var pendingDelete by remember { mutableStateOf<Collection<String>?>(null) }
    TorrentLoadState(state, onRetry, modifier, errorTitle = stringResource(R.string.torrent_library_error)) { items ->
        if (items.isEmpty()) {
            Column(modifier.fillMaxSize()) {
                header?.invoke()
                EmptyState(
                    icon = Icons.Outlined.DownloadForOffline,
                    title = stringResource(R.string.torrent_library_empty_title),
                    message = stringResource(R.string.torrent_library_empty_body),
                )
            }
        } else {
            Column(modifier.fillMaxSize()) {
                if (selection.isNotEmpty()) {
                    SelectionBar(
                        count = selection.size,
                        onSelectAll = { onSelectAll(items.map { it.id }) },
                        onPause = { onPause(selection) },
                        onResume = { onResume(selection) },
                        onKeep = { onPersist(selection, true) },
                        onDelete = { pendingDelete = selection },
                        onClear = onClearSelection,
                    )
                }
                LazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    if (header != null) item(key = "header", contentType = "header") { header() }
                    item(key = "count", contentType = "count") {
                        Text(
                            pluralStringResource(R.plurals.torrent_library_count, items.size, items.size),
                            style = MaterialTheme.typography.labelLarge,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    items(items, key = { it.id }, contentType = { "torrent" }) { item ->
                        LibraryCard(
                            item = item,
                            selecting = selection.isNotEmpty(),
                            selected = item.id in selection || item.id == selectedId,
                            checked = item.id in selection,
                            onClick = { if (selection.isNotEmpty()) onToggleSelect(item.id) else onOpen(item.id) },
                            onLongClick = { onToggleSelect(item.id) },
                            onOpen = { onOpen(item.id) },
                            onPlay = { onPlay(playArgsFor(item, item.id, null, item.episode, item.name)) },
                            onPause = { onPause(listOf(item.id)) },
                            onResume = { onResume(listOf(item.id)) },
                            onPersist = { onPersist(listOf(item.id), it) },
                            onDelete = { pendingDelete = listOf(item.id) },
                        )
                    }
                }
            }
        }
    }
    pendingDelete?.let { ids ->
        DeleteDialog(
            count = ids.size,
            onConfirm = { deleteFiles ->
                onRemove(ids, deleteFiles)
                pendingDelete = null
            },
            onDismiss = { pendingDelete = null },
        )
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun LibraryCard(
    item: TorrentLibraryItem,
    selecting: Boolean,
    selected: Boolean,
    checked: Boolean,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
    onOpen: () -> Unit,
    onPlay: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onPersist: (Boolean) -> Unit,
    onDelete: () -> Unit,
) {
    val context = LocalContext.current
    val title = item.series ?: item.name
    val selectLabel = stringResource(R.string.torrent_library_select, title)
    Surface(
        shape = MaterialTheme.shapes.large,
        color = if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainer,
        modifier = Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.large)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick, onLongClickLabel = selectLabel),
    ) {
        Row(Modifier.padding(12.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            if (item.poster != null) {
                MediaCover(url = item.poster, modifier = Modifier.width(52.dp), contentDescription = null)
            } else {
                Box(
                    Modifier
                        .size(52.dp)
                        .clip(MaterialTheme.shapes.medium)
                        .background(MaterialTheme.colorScheme.surfaceContainerHighest),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(Icons.Outlined.Movie, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                val detail = when {
                    item.episode != null && item.files <= 1 -> stringResource(R.string.torrent_library_episode, item.episode!!)
                    item.files > 1 -> pluralStringResource(R.plurals.torrent_library_batch, item.files, item.files)
                    else -> item.name
                }
                Text(
                    text = stringResource(R.string.torrent_library_detail, detail, formatBytes(item.sizeBytes)),
                    style = monoStyle(MaterialTheme.typography.bodySmall),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    StatePill(item.state)
                    Text(
                        text = stringResource(R.string.torrent_library_progress, percentText(item.progress)),
                        style = monoStyle(MaterialTheme.typography.labelMedium),
                    )
                    if (item.addedUnix > 0) {
                        Text(
                            text = DateUtils.getRelativeTimeSpanString(item.addedUnix * 1000).toString(),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 1,
                        )
                    }
                }
                Spacer(Modifier.height(2.dp))
                WavyProgress(progress = item.progress.toFloat(), modifier = Modifier.fillMaxWidth(), thin = true)
                Text(
                    text = stringResource(if (item.persist) R.string.torrent_library_kept else R.string.torrent_library_not_kept),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (selecting) {
                Checkbox(checked = checked, onCheckedChange = { onLongClick() }, modifier = Modifier.semantics { contentDescription = selectLabel })
            } else {
                var menu by remember { mutableStateOf(false) }
                Box {
                    IconButton(onClick = { menu = true }) {
                        Icon(Icons.Outlined.MoreVert, contentDescription = stringResource(R.string.torrent_library_actions))
                    }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.torrent_library_open)) },
                            leadingIcon = { Icon(Icons.Outlined.OpenInNew, null) },
                            onClick = { menu = false; onOpen() },
                        )
                        if (item.episode != null && item.mediaId != null) {
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.torrent_library_play)) },
                                leadingIcon = { Icon(Icons.Outlined.PlayArrow, null) },
                                onClick = { menu = false; onPlay() },
                            )
                        }
                        if (item.state == TorrentState.PAUSED) {
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.torrent_overview_resume)) },
                                leadingIcon = { Icon(Icons.Outlined.PlayArrow, null) },
                                onClick = { menu = false; onResume() },
                            )
                        } else {
                            DropdownMenuItem(
                                text = { Text(stringResource(R.string.torrent_overview_pause)) },
                                leadingIcon = { Icon(Icons.Outlined.Pause, null) },
                                onClick = { menu = false; onPause() },
                            )
                        }
                        DropdownMenuItem(
                            text = { Text(stringResource(if (item.persist) R.string.torrent_library_unkeep else R.string.torrent_library_keep)) },
                            leadingIcon = { Icon(if (item.persist) Icons.Outlined.Bookmark else Icons.Outlined.BookmarkBorder, null) },
                            onClick = { menu = false; onPersist(!item.persist) },
                        )
                        val shareTitle = stringResource(R.string.torrent_android_share_magnet)
                        DropdownMenuItem(
                            text = { Text(shareTitle) },
                            leadingIcon = { Icon(Icons.Outlined.Share, null) },
                            onClick = { menu = false; shareText(context, magnetOf(item.id), shareTitle) },
                        )
                        DropdownMenuItem(
                            text = { Text(stringResource(R.string.torrent_library_delete)) },
                            leadingIcon = { Icon(Icons.Outlined.Delete, null) },
                            onClick = { menu = false; onDelete() },
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun SelectionBar(
    count: Int,
    onSelectAll: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onKeep: () -> Unit,
    onDelete: () -> Unit,
    onClear: () -> Unit,
) {
    Surface(color = MaterialTheme.colorScheme.secondaryContainer, contentColor = MaterialTheme.colorScheme.onSecondaryContainer) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            IconButton(onClick = onClear) { Icon(Icons.Outlined.Close, contentDescription = stringResource(R.string.torrent_library_clear_selection)) }
            Text(
                pluralStringResource(R.plurals.torrent_library_selected, count, count),
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = onSelectAll) { Icon(Icons.Outlined.SelectAll, contentDescription = stringResource(R.string.torrent_library_select_all)) }
            IconButton(onClick = onPause) { Icon(Icons.Outlined.Pause, contentDescription = stringResource(R.string.torrent_overview_pause)) }
            IconButton(onClick = onResume) { Icon(Icons.Outlined.PlayArrow, contentDescription = stringResource(R.string.torrent_overview_resume)) }
            IconButton(onClick = onKeep) { Icon(Icons.Outlined.Bookmark, contentDescription = stringResource(R.string.torrent_library_keep)) }
            IconButton(onClick = onDelete) { Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.torrent_library_delete_selected)) }
        }
    }
}

/** Remove confirmation with "also delete the downloaded files" (desktop `torrent.library.delete_*`). */
@Composable
internal fun DeleteDialog(count: Int, onConfirm: (deleteFiles: Boolean) -> Unit, onDismiss: () -> Unit) {
    var deleteFiles by rememberSaveable { mutableStateOf(true) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = {
            Text(
                if (count == 1) stringResource(R.string.torrent_library_delete_one) else pluralStringResource(R.plurals.torrent_library_delete_many, count, count),
            )
        },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.torrent_library_delete_body))
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(checked = deleteFiles, onCheckedChange = { deleteFiles = it })
                    Text(stringResource(R.string.torrent_library_delete_files))
                }
                if (!deleteFiles) {
                    Text(
                        stringResource(R.string.torrent_library_delete_files_hint),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        },
        confirmButton = { TextButton(onClick = { onConfirm(deleteFiles) }) { Text(stringResource(R.string.torrent_library_delete)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(app.tatakai.next.ui.R.string.ui_close)) } },
    )
}
