// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.torrents.ui

import android.provider.OpenableColumns
import android.text.format.DateUtils
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AttachFile
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Key
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.feature.torrents.R
import app.tatakai.next.feature.torrents.vm.AddSourceKind
import app.tatakai.next.feature.torrents.vm.AddTorrentEvent
import app.tatakai.next.feature.torrents.vm.AddTorrentViewModel
import app.tatakai.next.feature.torrents.vm.DebridViewModel
import app.tatakai.next.feature.torrents.vm.TorrentDetailViewModel
import app.tatakai.next.feature.torrents.vm.TorrentHistoryEvent
import app.tatakai.next.feature.torrents.vm.TorrentHistoryViewModel
import app.tatakai.next.model.AppError
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.torrent.BandwidthSchedule
import app.tatakai.next.model.torrent.DebridAccount
import app.tatakai.next.model.torrent.DebridService
import app.tatakai.next.model.torrent.EpisodeAlignment
import app.tatakai.next.model.torrent.TorrentFile
import app.tatakai.next.model.torrent.TorrentSessionEntry
import app.tatakai.next.model.torrent.TorrentSettings
import app.tatakai.next.release.isVideoPath
import app.tatakai.next.torrent.policy.AlignFile
import app.tatakai.next.torrent.policy.alignedEpisodes
import app.tatakai.next.ui.format.formatBytes
import app.tatakai.next.ui.media.MediaCover
import org.koin.compose.viewmodel.koinViewModel
import java.text.DateFormat
import java.util.Date
import app.tatakai.next.ui.R as UiR

// parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.9, worker q): Añadir torrent, Alinear episodios, Historial,
// Debrid, re-announce and the new torrent settings.

// ==================================================================== Añadir torrent

/**
 * "Añadir torrent" (torrent.add-magnet / add-file / magnet-link-handler / episode-alignment): magnet or info-hash field, `.torrent` from
 * SAF, optional series (AniList search) and alignment (first episode / offset), "Guardar para después" (persisted download) switch.
 * [onAdded] receives the new torrent id (the client opens it).
 */
@Composable
internal fun AddTorrentSheet(vm: AddTorrentViewModel, onDismiss: () -> Unit, onAdded: (String) -> Unit) {
    val state by vm.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val currentAdded by rememberUpdatedState(onAdded)
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            when (event) {
                is AddTorrentEvent.Added -> currentAdded(event.id)
            }
        }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            val name = runCatching {
                context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
                    if (c.moveToFirst()) c.getString(0) else null
                }
            }.getOrNull()
            vm.setSource(uri.toString(), fileName = name ?: uri.lastPathSegment)
        }
    }
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Column(
            Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Text(stringResource(R.string.torrent_add_title), style = MaterialTheme.typography.titleLarge)
            if (state.kind == AddSourceKind.FILE) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Icon(Icons.Outlined.AttachFile, contentDescription = null)
                    Text(state.fileName ?: state.source, modifier = Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis)
                    IconButton(onClick = { vm.setSource("") }) {
                        Icon(Icons.Outlined.Close, contentDescription = stringResource(R.string.torrent_add_clear))
                    }
                }
            } else {
                OutlinedTextField(
                    value = state.source,
                    onValueChange = { vm.setSource(it) },
                    label = { Text(stringResource(R.string.torrent_add_source)) },
                    placeholder = { Text("magnet:?xt=urn:btih:…") },
                    isError = state.kind == AddSourceKind.INVALID,
                    supportingText = {
                        Text(
                            stringResource(
                                when (state.kind) {
                                    AddSourceKind.INVALID -> R.string.torrent_android_invalid_source
                                    AddSourceKind.HASH -> R.string.torrent_add_kind_hash
                                    AddSourceKind.MAGNET -> R.string.torrent_add_kind_magnet
                                    else -> R.string.torrent_add_source_hint
                                },
                            ),
                        )
                    },
                    minLines = 2,
                    maxLines = 4,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedButton(onClick = { picker.launch(arrayOf("application/x-bittorrent", "application/octet-stream", "*/*")) }) {
                    Icon(Icons.Outlined.AttachFile, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                    Text(stringResource(R.string.torrent_add_pick_file), modifier = Modifier.padding(start = 8.dp))
                }
            }

            HorizontalDivider()
            Text(stringResource(R.string.torrent_add_series), style = MaterialTheme.typography.titleSmall)
            Text(
                stringResource(R.string.torrent_add_series_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val media = state.media
            if (media != null) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    MediaCover(url = media.cover, modifier = Modifier.width(40.dp))
                    Text(media.title, modifier = Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis)
                    IconButton(onClick = { vm.pickMedia(null) }) {
                        Icon(Icons.Outlined.Close, contentDescription = stringResource(R.string.torrent_add_series_clear))
                    }
                }
            } else {
                OutlinedTextField(
                    value = state.seriesQuery,
                    onValueChange = vm::setSeriesQuery,
                    singleLine = true,
                    leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                    placeholder = { Text(stringResource(R.string.torrent_add_series_search)) },
                    modifier = Modifier.fillMaxWidth(),
                )
                when (val results = state.results) {
                    null -> Unit
                    LoadState.Loading -> Box(Modifier.fillMaxWidth().padding(8.dp), contentAlignment = Alignment.Center) { LoadingIndicator(Modifier.size(32.dp)) }
                    is LoadState.Err -> Text(torrentErrorMessage(results.cause), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                    is LoadState.Ok -> if (results.value.isEmpty()) {
                        Text(stringResource(R.string.torrent_add_series_none), style = MaterialTheme.typography.bodySmall)
                    } else {
                        Column {
                            results.value.forEach { m ->
                                ListItem(
                                    headlineContent = { Text(m.title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                                    supportingContent = m.year?.let { y -> { Text(y.toString()) } },
                                    leadingContent = { MediaCover(url = m.cover, modifier = Modifier.width(32.dp)) },
                                    modifier = Modifier.clickable { vm.pickMedia(m) },
                                )
                            }
                        }
                    }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                IntField(stringResource(R.string.torrent_align_first), state.firstEpisode, vm::setFirstEpisode, Modifier.weight(1f), allowNegative = false)
                IntField(stringResource(R.string.torrent_align_offset), state.offset, vm::setOffset, Modifier.weight(1f), allowNegative = true)
            }
            SwitchRow(
                title = stringResource(R.string.torrent_add_persist),
                hint = stringResource(R.string.torrent_add_persist_hint),
                checked = state.persist,
                onChange = vm::setPersist,
            )
            state.error?.let { Text(torrentErrorMessage(it), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium) }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
                TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_cancel)) }
                Button(
                    onClick = vm::add,
                    enabled = !state.adding && state.kind != AddSourceKind.EMPTY && state.kind != AddSourceKind.INVALID,
                    modifier = Modifier.padding(start = 8.dp),
                ) {
                    if (state.adding) LoadingIndicator(Modifier.size(20.dp))
                    Text(stringResource(if (state.adding) R.string.torrent_add_adding else R.string.torrent_add_confirm), modifier = Modifier.padding(start = 4.dp))
                }
            }
        }
    }
}

/** Integer field that saves every valid value (blank / "-" keep the last one). */
@Composable
private fun IntField(label: String, value: Int, onValue: (Int) -> Unit, modifier: Modifier = Modifier, allowNegative: Boolean) {
    var text by rememberSaveable(value) { mutableStateOf(value.toString()) }
    OutlinedTextField(
        value = text,
        onValueChange = { input ->
            val cleaned = input.filterIndexed { i, c -> c.isDigit() || (allowNegative && c == '-' && i == 0) }.take(5)
            text = cleaned
            cleaned.toIntOrNull()?.let(onValue)
        },
        label = { Text(label) },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = modifier,
    )
}

// ==================================================================== Alinear episodios (Files tab)

/**
 * Files › "Alinear episodios" (torrent.episode-alignment): proposes the episode of every video (high confidence when the name carries
 * the number, "by order" otherwise) and lets the user shift / renumber before saving the labels.
 */
@Composable
internal fun AlignmentSheet(files: List<TorrentFile>, vm: TorrentDetailViewModel, onDismiss: () -> Unit) {
    var first by rememberSaveable { mutableStateOf(1) }
    var offset by rememberSaveable { mutableStateOf(0) }
    val videos = remember(files) { files.filter { isVideoPath(it.path) } }
    // the parsed numbers come from the names, not from a previous alignment
    val parsed = remember(videos) { videos.map { AlignFile(it.index, it.path, app.tatakai.next.release.parseRelease(it.name).episodes?.first) } }
    val preview = remember(parsed, first, offset) { alignedEpisodes(parsed, EpisodeAlignment(first, offset)) }
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(stringResource(R.string.torrent_align_title), style = MaterialTheme.typography.titleLarge)
            Text(stringResource(R.string.torrent_align_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                IntField(stringResource(R.string.torrent_align_first), first, { first = it.coerceIn(1, 9_999) }, Modifier.weight(1f), allowNegative = false)
                IntField(stringResource(R.string.torrent_align_offset), offset, { offset = it.coerceIn(-9_999, 9_999) }, Modifier.weight(1f), allowNegative = true)
            }
            if (videos.isEmpty()) {
                EmptyState(icon = Icons.Outlined.Search, title = stringResource(R.string.torrent_align_no_videos))
            } else {
                LazyColumn(Modifier.fillMaxWidth().heightIn(max = 360.dp)) {
                    items(parsed.sortedBy { it.path.lowercase() }, key = { it.index }) { f ->
                        val ep = preview[f.index]
                        ListItem(
                            headlineContent = { Text(f.path.substringAfterLast('/'), maxLines = 2, overflow = TextOverflow.Ellipsis) },
                            supportingContent = {
                                Text(stringResource(if (f.episode != null) R.string.torrent_align_confidence_high else R.string.torrent_align_confidence_order))
                            },
                            trailingContent = {
                                Pill(
                                    if (ep != null) stringResource(R.string.torrent_files_episode_value, ep) else stringResource(R.string.torrent_fmt_none),
                                    tone = if (f.episode != null) PillTone.OK else PillTone.WARN,
                                )
                            },
                        )
                    }
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = {
                    vm.setAlignment(null)
                    onDismiss()
                }) { Text(stringResource(R.string.torrent_align_reset)) }
                Button(
                    onClick = {
                        vm.setAlignment(EpisodeAlignment(first, offset))
                        onDismiss()
                    },
                    enabled = videos.isNotEmpty(),
                    modifier = Modifier.padding(start = 8.dp),
                ) { Text(stringResource(R.string.torrent_align_save)) }
            }
        }
    }
}

// ==================================================================== Trackers: re-announce

@Composable
internal fun ReannounceButton(vm: TorrentDetailViewModel, modifier: Modifier = Modifier) {
    val busy by vm.reannouncing.collectAsStateWithLifecycle()
    FilledTonalButton(onClick = vm::reannounce, enabled = !busy, modifier = modifier) {
        if (busy) LoadingIndicator(Modifier.size(20.dp)) else Icon(Icons.Outlined.Bolt, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
        Text(stringResource(R.string.torrent_trackers_reannounce), modifier = Modifier.padding(start = 8.dp))
    }
}

// ==================================================================== Historial

/** TorrentSection.HISTORY (torrent.session-history): torrents the session has seen, with date; tap reopens; "Vaciar". */
@Composable
internal fun HistorySection(onOpenTorrent: (String) -> Unit, modifier: Modifier = Modifier) {
    val vm: TorrentHistoryViewModel = koinViewModel()
    val state by vm.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val failed = stringResource(R.string.torrent_library_action_failed)
    val currentOpen by rememberUpdatedState(onOpenTorrent)
    var confirm by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(vm) {
        vm.events.collect { e ->
            when (e) {
                is TorrentHistoryEvent.Open -> currentOpen(e.id)
                is TorrentHistoryEvent.Failed -> snackbar.show(failed)
            }
        }
    }
    TorrentLoadState(state, vm::retry, modifier, errorTitle = stringResource(R.string.torrent_history_error)) { entries ->
        if (entries.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.History,
                title = stringResource(R.string.torrent_history_empty_title),
                message = stringResource(R.string.torrent_history_empty_body),
                modifier = modifier,
            )
        } else {
            Column(modifier) {
                Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(R.string.torrent_history_count, entries.size),
                        style = monoStyle(MaterialTheme.typography.labelMedium),
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = { confirm = true }) {
                        Icon(Icons.Outlined.DeleteSweep, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                        Text(stringResource(R.string.torrent_history_clear), modifier = Modifier.padding(start = 4.dp))
                    }
                }
                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                    items(entries, key = { it.id }) { entry -> HistoryRow(entry) { vm.reopen(entry) } }
                }
            }
        }
    }
    if (confirm) {
        AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.torrent_history_clear_title)) },
            text = { Text(stringResource(R.string.torrent_history_clear_body)) },
            confirmButton = {
                TextButton(onClick = {
                    confirm = false
                    vm.clear()
                }) { Text(stringResource(R.string.torrent_history_clear)) }
            },
            dismissButton = { TextButton(onClick = { confirm = false }) { Text(stringResource(UiR.string.ui_cancel)) } },
        )
    }
}

@Composable
private fun HistoryRow(entry: TorrentSessionEntry, onClick: () -> Unit) {
    val added = DateUtils.getRelativeTimeSpanString(entry.addedUnix * 1000, System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS).toString()
    ListItem(
        modifier = Modifier.clickable(onClick = onClick),
        headlineContent = { Text(entry.series ?: entry.name, maxLines = 2, overflow = TextOverflow.Ellipsis) },
        supportingContent = {
            Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                if (entry.series != null && entry.name != entry.series) {
                    Text(entry.name, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
                Text(
                    listOfNotNull(
                        entry.episode?.let { stringResource(R.string.torrent_files_episode_value, it) },
                        added,
                        entry.downloadedBytes.takeIf { it > 0 }?.let { formatBytes(it) },
                    ).joinToString(" · "),
                    style = monoStyle(MaterialTheme.typography.bodySmall),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        },
        trailingContent = {
            Column(horizontalAlignment = Alignment.End, verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Pill(stringResource(originRes(entry.origin)), tone = PillTone.NEUTRAL)
                if (entry.removedUnix != null) Pill(stringResource(R.string.torrent_history_removed), tone = PillTone.WARN)
            }
        },
    )
}

private fun originRes(origin: String): Int = when (origin) {
    "download" -> R.string.torrent_history_origin_download
    "manual" -> R.string.torrent_history_origin_manual
    "magnet" -> R.string.torrent_history_origin_magnet
    else -> R.string.torrent_history_origin_play
}

// ==================================================================== Debrid

/**
 * TorrentSection.DEBRID (sources.debrid.settings): one card per service with the API key field, "Probar y vincular" (validates against
 * the account endpoint, key stored in the Keystore, never shown again), user / premium, "Preferir" (order) and "Desvincular".
 */
@Composable
internal fun DebridSection(modifier: Modifier = Modifier) {
    val vm: DebridViewModel = koinViewModel()
    val state by vm.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val context = LocalContext.current
    LaunchedEffect(vm) {
        vm.linked.collect { a -> snackbar.show(context.getString(R.string.torrent_debrid_linked, a.service.label, a.username ?: a.service.label)) }
    }
    if (!vm.available) {
        ErrorState(
            message = stringResource(R.string.torrent_debrid_unavailable),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = {},
            modifier = modifier,
        )
        return
    }
    Column(
        modifier.verticalScroll(rememberScrollState()).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text(stringResource(R.string.torrent_debrid_intro), style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        val ordered = state.accounts.map { it.service } + DebridService.entries.filter { s -> state.accounts.none { it.service == s } }
        ordered.forEach { service ->
            val account = state.accounts.firstOrNull { it.service == service }
            DebridCard(
                service = service,
                account = account,
                position = state.accounts.indexOfFirst { it.service == service }.takeIf { it >= 0 },
                linking = state.linking == service,
                error = state.errors[service],
                onLink = { key -> vm.link(service, key) },
                onUnlink = { vm.unlink(service) },
                onPrefer = { vm.prefer(service) },
            )
        }
        Text(stringResource(R.string.torrent_debrid_note), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun DebridCard(
    service: DebridService,
    account: DebridAccount?,
    position: Int?,
    linking: Boolean,
    error: AppError?,
    onLink: (String) -> Unit,
    onUnlink: () -> Unit,
    onPrefer: () -> Unit,
) {
    var key by remember(service, account) { mutableStateOf("") }
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = MaterialTheme.shapes.large, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(service.label, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
                if (position != null) Pill(stringResource(R.string.torrent_debrid_order, position + 1), tone = if (position == 0) PillTone.PRIMARY else PillTone.NEUTRAL)
                Pill(
                    stringResource(if (account != null) R.string.torrent_debrid_linked_pill else R.string.torrent_debrid_not_linked),
                    tone = if (account != null) PillTone.OK else PillTone.NEUTRAL,
                )
            }
            Text(
                stringResource(if (service.instantCheck()) R.string.torrent_debrid_instant else R.string.torrent_debrid_probe),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (account != null) {
                Text(stringResource(R.string.torrent_debrid_user, account.username ?: "—"), style = MaterialTheme.typography.bodyMedium)
                Text(
                    account.premiumUntilMs?.let {
                        stringResource(R.string.torrent_debrid_premium_until, DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(it)))
                    } ?: stringResource(R.string.torrent_debrid_premium_unknown),
                    style = MaterialTheme.typography.bodySmall,
                    color = if ((account.premiumUntilMs ?: Long.MAX_VALUE) < System.currentTimeMillis()) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurfaceVariant,
                )
                Text(stringResource(R.string.torrent_debrid_key_saved), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (position != null && position > 0) OutlinedButton(onClick = onPrefer) { Text(stringResource(R.string.torrent_debrid_prefer)) }
                    TextButton(onClick = onUnlink, colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error)) {
                        Text(stringResource(R.string.torrent_debrid_unlink))
                    }
                }
            } else {
                OutlinedTextField(
                    value = key,
                    onValueChange = { key = it.trim() },
                    label = { Text(stringResource(R.string.torrent_debrid_key)) },
                    singleLine = true,
                    leadingIcon = { Icon(Icons.Outlined.Key, contentDescription = null) },
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    isError = error != null,
                    supportingText = error?.let { e -> { Text(debridErrorText(e)) } },
                    modifier = Modifier.fillMaxWidth(),
                )
                FilledTonalButton(onClick = { onLink(key) }, enabled = key.isNotBlank() && !linking) {
                    if (linking) LoadingIndicator(Modifier.size(20.dp))
                    Text(stringResource(R.string.torrent_debrid_test_link), modifier = Modifier.padding(start = 4.dp))
                }
            }
        }
    }
}

private fun DebridService.instantCheck(): Boolean = this == DebridService.TORBOX || this == DebridService.PREMIUMIZE

@Composable
private fun debridErrorText(error: AppError): String {
    val msg = (error as? AppError.Unknown)?.msg.orEmpty()
    return when {
        msg == "debrid:invalid_key" -> stringResource(R.string.torrent_debrid_invalid_key)
        error is AppError.Offline -> stringResource(R.string.torrent_debrid_offline)
        else -> torrentErrorMessage(error)
    }
}

// ==================================================================== settings: schedule, trackers, cache, preload

/**
 * New rows of Settings › Torrents (torrent.bandwidth-schedule, torrent.custom-trackers, torrent.cache-limit, torrent.preload-next).
 * Saved through the same [onChange] as the rest of the page (inside `torrent_settings_json`).
 */
@Composable
internal fun ParityTorrentSettings(settings: TorrentSettings, onChange: ((TorrentSettings) -> TorrentSettings) -> Unit) {
    SectionCard(title = stringResource(R.string.torrent_sched_title)) {
        val schedule = settings.schedule
        SwitchRow(
            title = stringResource(R.string.torrent_sched_switch),
            hint = stringResource(R.string.torrent_sched_hint),
            checked = schedule != null,
            onChange = { on -> onChange { it.copy(schedule = if (on) BandwidthSchedule(startMinute = 8 * 60, endMinute = 23 * 60, downloadLimitKib = 1_024) else null) } },
        )
        if (schedule != null) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                TimeField(stringResource(R.string.torrent_sched_start), schedule.startMinute, Modifier.weight(1f)) { m ->
                    onChange { it.copy(schedule = it.schedule?.copy(startMinute = m)) }
                }
                TimeField(stringResource(R.string.torrent_sched_end), schedule.endMinute, Modifier.weight(1f)) { m ->
                    onChange { it.copy(schedule = it.schedule?.copy(endMinute = m)) }
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                IntField(stringResource(R.string.torrent_sched_down), schedule.downloadLimitKib, { v ->
                    onChange { it.copy(schedule = it.schedule?.copy(downloadLimitKib = v.coerceIn(0, TorrentSettings.MAX_LIMIT_KIB))) }
                }, Modifier.weight(1f), allowNegative = false)
                IntField(stringResource(R.string.torrent_sched_up), schedule.uploadLimitKib, { v ->
                    onChange { it.copy(schedule = it.schedule?.copy(uploadLimitKib = v.coerceIn(0, TorrentSettings.MAX_LIMIT_KIB))) }
                }, Modifier.weight(1f), allowNegative = false)
            }
            Text(stringResource(R.string.torrent_sched_limits_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
    SectionCard(title = stringResource(R.string.torrent_trackers_custom_title)) {
        var text by rememberSaveable(settings.trackers) { mutableStateOf(settings.trackers.joinToString("\n")) }
        OutlinedTextField(
            value = text,
            onValueChange = { text = it },
            label = { Text(stringResource(R.string.torrent_trackers_custom_label)) },
            supportingText = { Text(stringResource(R.string.torrent_trackers_custom_hint)) },
            minLines = 3,
            maxLines = 8,
            modifier = Modifier.fillMaxWidth(),
        )
        val parsed = text.lines().map(String::trim).filter(String::isNotEmpty)
        OutlinedButton(onClick = { onChange { it.copy(trackers = parsed) } }, enabled = parsed != settings.trackers) {
            Text(stringResource(R.string.torrent_trackers_custom_save))
        }
    }
    SectionCard(title = stringResource(R.string.torrent_cache_title)) {
        SwitchRow(
            title = stringResource(R.string.torrent_cache_switch),
            hint = stringResource(R.string.torrent_cache_hint),
            checked = settings.cacheLimitMib > 0,
            onChange = { on -> onChange { it.copy(cacheLimitMib = if (on) DEFAULT_CACHE_MIB else 0) } },
        )
        if (settings.cacheLimitMib > 0) {
            IntField(
                stringResource(R.string.torrent_cache_gib),
                (settings.cacheLimitMib / 1024).coerceAtLeast(1),
                { gib -> onChange { it.copy(cacheLimitMib = (gib.coerceAtLeast(1) * 1024).coerceAtMost(TorrentSettings.MAX_CACHE_MIB)) } },
                Modifier.fillMaxWidth(),
                allowNegative = false,
            )
        }
        SwitchRow(
            title = stringResource(R.string.torrent_preload_switch),
            hint = stringResource(R.string.torrent_preload_hint),
            checked = settings.preloadNext,
            onChange = { on -> onChange { it.copy(preloadNext = on) } },
        )
    }
}

/** `HH:MM` field (24 h) for the schedule window. */
@Composable
private fun TimeField(label: String, minute: Int, modifier: Modifier, onMinute: (Int) -> Unit) {
    var text by rememberSaveable(minute) { mutableStateOf(formatMinute(minute)) }
    val parsed = parseMinute(text)
    OutlinedTextField(
        value = text,
        onValueChange = { input ->
            text = input.filter { it.isDigit() || it == ':' }.take(5)
            parseMinute(text)?.let { if (it != minute) onMinute(it) }
        },
        label = { Text(label) },
        singleLine = true,
        isError = parsed == null,
        placeholder = { Text("23:00") },
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
        modifier = modifier,
    )
}

/** 0..1439 -> `HH:MM`. */
internal fun formatMinute(minute: Int): String = "%02d:%02d".format(minute.coerceIn(0, 1439) / 60, minute.coerceIn(0, 1439) % 60)

/** `HH:MM` / `H:MM` / `HHMM` -> 0..1439; null when invalid. Pure, unit tested. */
internal fun parseMinute(text: String): Int? {
    val t = text.trim()
    val (h, m) = when {
        ':' in t -> t.substringBefore(':') to t.substringAfter(':')
        t.length in 3..4 -> t.dropLast(2) to t.takeLast(2)
        else -> return null
    }
    val hour = h.toIntOrNull() ?: return null
    val min = m.toIntOrNull() ?: return null
    if (hour !in 0..23 || min !in 0..59 || m.length != 2) return null
    return hour * 60 + min
}

private const val DEFAULT_CACHE_MIB = 10 * 1024
