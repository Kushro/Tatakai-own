// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents.ui

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.ui.draw.clip
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
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.BatteryAlert
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.DownloadForOffline
import androidx.compose.material.icons.outlined.Pause
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.ReportProblem
import androidx.compose.material.icons.outlined.SignalWifiOff
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.WavyProgress
import app.tatakai.next.feature.torrents.R
import app.tatakai.next.feature.torrents.vm.DownloadsUi
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.model.torrent.TorrentState
import app.tatakai.next.model.torrent.TorrentStats
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.formatBytes
import app.tatakai.next.ui.format.formatRate
import kotlinx.coroutines.flow.catch
import org.koin.compose.koinInject

/** true when POST_NOTIFICATIONS must still be requested (Android 13+). */
@Composable
internal fun rememberNeedsNotificationPermission(): () -> Boolean {
    val context = LocalContext.current
    return remember(context) {
        {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        }
    }
}

/**
 * The "Entiendo / Ahora no" warning (11 §3.4): full screen, not a dismissable dialog. Explains CPU, battery, storage and data use,
 * uploading to peers (IP visible) and the permanent notification, plus why notifications are requested.
 */
@Composable
internal fun BackgroundWarning(onAccept: () -> Unit, onNotNow: () -> Unit, modifier: Modifier = Modifier) {
    Column(
        modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp)
            .navigationBarsPadding(),
        verticalArrangement = Arrangement.spacedBy(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(Icons.Outlined.WarningAmber, contentDescription = null, modifier = Modifier.size(64.dp), tint = MaterialTheme.colorScheme.tertiary)
        Text(
            stringResource(R.string.torrent_downloads_warn_title),
            style = MaterialTheme.typography.headlineSmall,
            textAlign = TextAlign.Center,
        )
        Column(Modifier.widthIn(max = 560.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(stringResource(R.string.torrent_downloads_warn_body), style = MaterialTheme.typography.bodyLarge)
            listOf(
                R.string.torrent_downloads_warn_cpu,
                R.string.torrent_android_warn_battery,
                R.string.torrent_downloads_warn_disk,
                R.string.torrent_downloads_warn_network,
                R.string.torrent_downloads_warn_upload,
                R.string.torrent_android_warn_notification,
            ).forEach { res ->
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier
                            .size(6.dp)
                            .clip(CircleShape)
                            .background(MaterialTheme.colorScheme.onSurfaceVariant),
                    )
                    Text(stringResource(res), style = MaterialTheme.typography.bodyMedium)
                }
            }
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                Text(
                    stringResource(R.string.torrent_android_notif_rationale),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Text(
                stringResource(R.string.torrent_android_warn_note),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Button(onClick = onAccept, modifier = Modifier.fillMaxWidth().widthIn(max = 560.dp)) {
            Text(stringResource(R.string.torrent_downloads_warn_accept))
        }
        TextButton(onClick = onNotNow) { Text(stringResource(R.string.torrent_android_not_now)) }
    }
}

/** Body of Downloads (11 §3.4) after the acknowledgement: switch, network / battery chips, list of unfinished downloads. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun DownloadsBody(
    ui: DownloadsUi,
    onAccept: () -> Unit,
    onNotNow: () -> Unit,
    onBackground: (Boolean) -> Unit,
    onOpenTorrent: (String) -> Unit,
    onPause: (String) -> Unit,
    onResume: (String) -> Unit,
    onRemove: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val needsPermission = rememberNeedsNotificationPermission()
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    if (!ui.settings.backgroundAck) {
        BackgroundWarning(
            onAccept = {
                onAccept()
                if (needsPermission()) permission.launch(Manifest.permission.POST_NOTIFICATIONS)
            },
            onNotNow = onNotNow,
            modifier = modifier,
        )
        return
    }
    var confirmRemove by remember { mutableStateOf<String?>(null) }
    LazyColumn(modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "switch") {
            SwitchRow(
                title = stringResource(R.string.torrent_downloads_switch),
                hint = stringResource(R.string.torrent_android_downloads_switch_hint),
                checked = ui.settings.backgroundDownloads,
                onChange = { enabled ->
                    onBackground(enabled)
                    if (enabled && needsPermission()) permission.launch(Manifest.permission.POST_NOTIFICATIONS)
                },
            )
        }
        item(key = "chips") {
            FlowRow(
                Modifier.fillMaxWidth().padding(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                if (ui.status.waitingForWifi) {
                    AssistChip(
                        onClick = {},
                        label = { Text(stringResource(R.string.torrent_android_waiting_wifi)) },
                        leadingIcon = { Icon(Icons.Outlined.SignalWifiOff, null) },
                    )
                }
                if (ui.status.pausedLowBattery) {
                    AssistChip(
                        onClick = {},
                        label = { Text(stringResource(R.string.torrent_android_low_battery)) },
                        leadingIcon = { Icon(Icons.Outlined.BatteryAlert, null) },
                    )
                }
                if (ui.status.pausedBySystem) {
                    AssistChip(
                        onClick = { ui.items.forEach { onResume(it.id) } },
                        label = { Text(stringResource(R.string.torrent_android_paused_by_system)) },
                        leadingIcon = { Icon(Icons.Outlined.ReportProblem, null) },
                    )
                }
            }
        }
        if (!ui.settings.backgroundDownloads) {
            item(key = "off") {
                Banner(stringResource(R.string.torrent_downloads_off_banner), Modifier.padding(16.dp))
            }
        }
        if (ui.items.isEmpty()) {
            item(key = "empty") {
                EmptyState(
                    icon = Icons.Outlined.DownloadForOffline,
                    title = stringResource(R.string.torrent_downloads_empty_title),
                    message = stringResource(
                        if (ui.settings.backgroundDownloads) R.string.torrent_downloads_empty_body else R.string.torrent_downloads_empty_body_off,
                    ),
                )
            }
        } else {
            items(ui.items, key = { it.id }, contentType = { "download" }) { item ->
                DownloadRow(
                    item = item,
                    onOpen = { onOpenTorrent(item.id) },
                    onPause = { onPause(item.id) },
                    onResume = { onResume(item.id) },
                    onRemove = { confirmRemove = item.id },
                )
            }
        }
    }
    confirmRemove?.let { id ->
        AlertDialog(
            onDismissRequest = { confirmRemove = null },
            title = { Text(stringResource(R.string.torrent_downloads_remove_title)) },
            text = { Text(stringResource(R.string.torrent_library_delete_body)) },
            confirmButton = {
                TextButton(onClick = {
                    onRemove(id)
                    confirmRemove = null
                }) { Text(stringResource(R.string.torrent_downloads_remove)) }
            },
            dismissButton = { TextButton(onClick = { confirmRemove = null }) { Text(stringResource(UiR.string.ui_close)) } },
        )
    }
}

@Composable
private fun DownloadRow(
    item: TorrentLibraryItem,
    onOpen: () -> Unit,
    onPause: () -> Unit,
    onResume: () -> Unit,
    onRemove: () -> Unit,
) {
    // live figures of this row (the repository ticks every second while collected)
    val repository = koinInject<TorrentRepository>()
    val statsFlow = remember(item.id) { repository.stats(item.id).catch { } }
    val stats by statsFlow.collectAsStateWithLifecycle(initialValue = null as TorrentStats?)
    Surface(
        onClick = onOpen,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surfaceContainer,
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(item.series ?: item.name, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    item.episode?.let {
                        Text(
                            stringResource(R.string.torrent_downloads_episode, it),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                val state = stats?.state ?: item.state
                if (state == TorrentState.PAUSED) {
                    IconButton(onClick = onResume) { Icon(Icons.Outlined.PlayArrow, stringResource(R.string.torrent_downloads_resume)) }
                } else {
                    IconButton(onClick = onPause) { Icon(Icons.Outlined.Pause, stringResource(R.string.torrent_downloads_pause)) }
                }
                IconButton(onClick = onRemove) { Icon(Icons.Outlined.Delete, stringResource(R.string.torrent_downloads_remove)) }
            }
            val progress = stats?.progress ?: item.progress
            WavyProgress(progress = progress.toFloat(), modifier = Modifier.fillMaxWidth(), thin = true)
            val s = stats
            val line = buildList {
                add(
                    stringResource(
                        R.string.torrent_downloads_of,
                        formatBytes(s?.downloadedBytes ?: (item.sizeBytes * item.progress).toLong()),
                        formatBytes(s?.totalBytes ?: item.sizeBytes),
                    ),
                )
                if (s != null) {
                    add(stringResource(R.string.torrent_downloads_down, formatRate(s.downBps)))
                    add(stringResource(R.string.torrent_downloads_up, formatRate(s.upBps)))
                    add(pluralStringResource(R.plurals.torrent_downloads_peers, s.peers, s.peers))
                    s.etaSecs?.let { add(stringResource(R.string.torrent_downloads_eta, durationText(it))) }
                }
            }.joinToString(" · ")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                StatePill(s?.state ?: item.state)
                Text(line, style = monoStyle(MaterialTheme.typography.bodySmall), maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
    }
}
