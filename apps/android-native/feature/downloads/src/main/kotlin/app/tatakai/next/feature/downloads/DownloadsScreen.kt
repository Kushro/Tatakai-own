// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import android.Manifest
import android.content.pm.PackageManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.AutoStories
import androidx.compose.material.icons.outlined.CloudDownload
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material.icons.outlined.Pause
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.VerifiedUser
import androidx.compose.material.icons.outlined.VideoFile
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ListGroupHeader
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppException
import app.tatakai.next.model.AutoDownloadRule
import app.tatakai.next.model.ChapterDownload
import app.tatakai.next.model.ChapterKind
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadProfile
import app.tatakai.next.model.DownloadState
import app.tatakai.next.model.DownloadsTab
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LocalLibraryItem
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.torrent.TorrentLibraryItem
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.formatBytes
import app.tatakai.next.ui.format.formatDuration
import app.tatakai.next.ui.labels.labelRes
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.state.toAppError
import app.tatakai.next.ui.state.userMessage
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import org.koin.compose.getKoin
import org.koin.compose.viewmodel.koinViewModel

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r; checklist downloads.page / .history /
 * .local-library.* / .integrity-check / .storage-limit): `Route.Downloads(tab)`. Tabs = `DownloadsTab` (Activas · Anime · Manga · Novelas ·
 * Historial · Biblioteca local); search field; rows with progress, ETA, pause / resume / cancel / retry; torrents rows open the client
 * ([onOpenTorrent]); completed episodes play offline ([onPlay] with `PlayerArgs.localUri`); chapters open their reader offline.
 * [onBack] null on wide windows (rail destination). Signature FINAL (the shell wires it).
 */
@OptIn(ExperimentalMaterial3Api::class)
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
    val state by viewModel.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val context = LocalContext.current
    LaunchedEffect(initialTab) { viewModel.selectTab(initialTab) }
    // Downloads notify (progress / done): ask POST_NOTIFICATIONS here, never at app start (16 §0.12).
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    LaunchedEffect(Unit) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            permission.launch(Manifest.permission.POST_NOTIFICATIONS)
        }
    }
    val msgChecked = stringResource(R.string.downloads_checked)
    val msgVerified = stringResource(R.string.downloads_verified)
    val msgVerifiedOk = stringResource(R.string.downloads_verified_ok)
    val msgImported = stringResource(R.string.downloads_imported)
    val msgPreviewNone = stringResource(R.string.downloads_preview_none)
    val msgPreview = stringResource(R.string.downloads_preview)
    val errorText = rememberDownloadErrorText()
    LaunchedEffect(viewModel) {
        viewModel.events.collect { e ->
            when (e) {
                is DownloadsEvent.Checked -> snackbar.show(msgChecked.format(e.queued))
                is DownloadsEvent.Verified -> snackbar.show(if (e.broken == 0) msgVerifiedOk else msgVerified.format(e.broken))
                is DownloadsEvent.Imported -> snackbar.show(msgImported.format(e.added))
                is DownloadsEvent.Preview -> snackbar.show(
                    e.preview?.episodes?.takeIf { it.isNotEmpty() }?.let { msgPreview.format(it.joinToString(", ")) } ?: msgPreviewNone,
                )
                is DownloadsEvent.Failed -> snackbar.show(errorText(e.cause))
            }
        }
    }
    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.downloads_title)) },
                navigationIcon = {
                    if (onBack != null) {
                        IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) }
                    }
                },
                actions = {
                    IconButton(onClick = viewModel::verify, enabled = !state.busy) {
                        Icon(Icons.Outlined.VerifiedUser, contentDescription = stringResource(R.string.downloads_verify))
                    }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            OutlinedTextField(
                value = state.query,
                onValueChange = viewModel::setQuery,
                leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                placeholder = { Text(stringResource(R.string.downloads_search)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
            )
            PrimaryScrollableTabRow(selectedTabIndex = DownloadsTab.entries.indexOf(state.tab)) {
                DownloadsTab.entries.forEach { tab ->
                    Tab(selected = state.tab == tab, onClick = { viewModel.selectTab(tab) }, text = { Text(stringResource(tab.labelRes())) })
                }
            }
            if (state.busy) LinearProgressIndicator(Modifier.fillMaxWidth())
            if (!state.loaded) {
                if (rememberAfterDelay()) LoadingState()
                return@Column
            }
            val play: (DownloadItem) -> Unit = { item ->
                item.localUri?.let { onPlay(PlayerArgs(media = item.ref, title = item.title, episode = item.episode, localUri = it)) }
            }
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                state.usedBytes?.let { used ->
                    item(key = "usage") {
                        Text(
                            stringResource(R.string.downloads_usage, formatBytes(used)),
                            style = MaterialTheme.typography.labelMedium,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                        )
                    }
                }
                when (state.tab) {
                    DownloadsTab.ACTIVE -> activeTab(state, viewModel, onOpenTorrent, errorText)
                    DownloadsTab.ANIME -> animeTab(state, viewModel, play, onOpenDetails, onOpenTorrent)
                    DownloadsTab.MANGA -> chaptersTab(state.manga, ChapterKind.MANGA, viewModel) { c ->
                        decodeArgs(c, MangaReaderArgs.serializer())?.let(onOpenMangaReader) ?: snackbar.show(context.getString(R.string.downloads_chapter_unavailable))
                    }
                    DownloadsTab.NOVELS -> chaptersTab(state.novels, ChapterKind.NOVEL, viewModel) { c ->
                        decodeArgs(c, NovelReaderArgs.serializer())?.let(onOpenNovelReader) ?: snackbar.show(context.getString(R.string.downloads_chapter_unavailable))
                    }
                    DownloadsTab.HISTORY -> historyTab(state, viewModel, play, errorText)
                    DownloadsTab.LOCAL -> localTab(state, viewModel, onPlay)
                }
            }
        }
    }
}

private val ArgsJson = Json { ignoreUnknownKeys = true }

/** `ChapterDownload.localRef` = JSON of the reader args written by s (offline = true). */
private fun <T> decodeArgs(c: ChapterDownload, serializer: kotlinx.serialization.KSerializer<T>): T? =
    runCatching { ArgsJson.decodeFromString(serializer, c.localRef) }.getOrNull()

internal fun DownloadsTab.labelRes(): Int = when (this) {
    DownloadsTab.ACTIVE -> R.string.downloads_tab_active
    DownloadsTab.ANIME -> R.string.downloads_tab_anime
    DownloadsTab.MANGA -> R.string.downloads_tab_manga
    DownloadsTab.NOVELS -> R.string.downloads_tab_novels
    DownloadsTab.HISTORY -> R.string.downloads_tab_history
    DownloadsTab.LOCAL -> R.string.downloads_tab_local
}

/** Localised text of a download failure (the short codes of `DownloadRepositoryImpl` + the shared [userMessage]). */
@Composable
internal fun rememberDownloadErrorText(): (Throwable) -> String {
    val dash = stringResource(R.string.downloads_err_dash)
    val limit = stringResource(R.string.downloads_err_limit)
    val noSource = stringResource(R.string.downloads_err_no_source)
    val folder = stringResource(R.string.downloads_err_folder)
    val generic = stringResource(R.string.downloads_err_generic)
    return remember(dash, limit, noSource, folder, generic) {
        { t: Throwable ->
            val code = ((t as? AppException)?.error as? AppError.Extension)?.msg ?: (t.toAppError() as? AppError.Unknown)?.msg
            codeText(code, dash, limit, noSource, folder) ?: generic.format(t.message ?: t::class.java.simpleName)
        }
    }
}

internal fun codeText(code: String?, dash: String, limit: String, noSource: String, folder: String): String? = when (code) {
    "dash_not_supported" -> dash
    "storage_limit" -> limit
    "no_source" -> noSource
    "folder_unavailable" -> folder
    else -> null
}

@Composable
private fun itemErrorText(item: DownloadItem): String? {
    val err = item.error ?: return null
    return when {
        item.state == DownloadState.BROKEN || err == "broken" -> stringResource(R.string.downloads_err_broken)
        err == "failed" -> stringResource(R.string.downloads_err_failed)
        else -> err
    }
}

// ------------------------------------------------------------------ Activas

private fun LazyListScope.activeTab(
    state: DownloadsUiState,
    vm: DownloadsViewModel,
    onOpenTorrent: (String) -> Unit,
    @Suppress("UNUSED_PARAMETER") errorText: (Throwable) -> String,
) {
    if (state.active.isEmpty() && state.activeTorrents.isEmpty()) {
        item(key = "empty") {
            EmptyState(icon = Icons.Outlined.CloudDownload, title = stringResource(R.string.downloads_empty_active), message = stringResource(R.string.downloads_empty_active_body))
        }
        return
    }
    items(state.active, key = { "w:" + it.id }) { item -> ActiveRow(item, state.eta[item.id], vm) }
    if (state.activeTorrents.isNotEmpty()) {
        item(key = "t-header") { ListGroupHeader(stringResource(R.string.downloads_torrents)) }
        items(state.activeTorrents, key = { "t:" + it.id }) { t -> TorrentRow(t, onOpenTorrent, vm) }
    }
}

@Composable
private fun ActiveRow(item: DownloadItem, eta: Long?, vm: DownloadsViewModel) {
    var confirm by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
        MediaCover(url = item.poster, modifier = Modifier.width(44.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(item.title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOfNotNull(
                    stringResource(UiR.string.ui_episode_number, item.episode),
                    item.sourceLabel.takeIf { it.isNotBlank() },
                    stringResource(item.state.labelRes()),
                    eta?.let { stringResource(R.string.downloads_eta, formatDuration(it)) },
                ).joinToString(" · "),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (item.state == DownloadState.FAILED) {
                itemErrorText(item)?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
            } else {
                LinearProgressIndicator(progress = { item.progress.toFloat().coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth())
                Text(
                    "${(item.progress * 100).toInt()}% · ${formatBytes(item.downloadedBytes)}" + (item.totalBytes?.let { " / " + formatBytes(it) } ?: ""),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        when (item.state) {
            DownloadState.PAUSED -> IconButton(onClick = { vm.resume(item.id) }) { Icon(Icons.Filled.PlayArrow, contentDescription = stringResource(R.string.downloads_resume)) }
            DownloadState.FAILED -> IconButton(onClick = { vm.retry(item.id) }) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(UiR.string.ui_retry)) }
            else -> IconButton(onClick = { vm.pause(item.id) }) { Icon(Icons.Outlined.Pause, contentDescription = stringResource(R.string.downloads_pause)) }
        }
        IconButton(onClick = { confirm = true }) { Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.downloads_cancel)) }
    }
    if (confirm) {
        ConfirmDialog(
            title = stringResource(R.string.downloads_cancel),
            text = stringResource(R.string.downloads_cancel_confirm, item.title, item.episode),
            onConfirm = { vm.remove(item.id) },
            onDismiss = { confirm = false },
        )
    }
}

@Composable
private fun TorrentRow(t: TorrentLibraryItem, onOpen: (String) -> Unit, vm: DownloadsViewModel) {
    Row(
        Modifier.fillMaxWidth().clickable { onOpen(t.id) }.padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaCover(url = t.poster, modifier = Modifier.width(44.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(t.series ?: t.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOfNotNull(t.episode?.let { stringResource(UiR.string.ui_episode_number, it) }, stringResource(R.string.downloads_torrent), formatBytes(t.sizeBytes)).joinToString(" · "),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            if (t.progress < 1.0) LinearProgressIndicator(progress = { t.progress.toFloat().coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth())
        }
        if (t.progress < 1.0) {
            if (t.state == app.tatakai.next.model.torrent.TorrentState.PAUSED) {
                IconButton(onClick = { vm.resumeTorrent(t.id) }) { Icon(Icons.Filled.PlayArrow, contentDescription = stringResource(R.string.downloads_resume)) }
            } else {
                IconButton(onClick = { vm.pauseTorrent(t.id) }) { Icon(Icons.Outlined.Pause, contentDescription = stringResource(R.string.downloads_pause)) }
            }
        }
    }
}

// ------------------------------------------------------------------ Anime

private fun LazyListScope.animeTab(
    state: DownloadsUiState,
    vm: DownloadsViewModel,
    play: (DownloadItem) -> Unit,
    onOpenDetails: (Int) -> Unit,
    onOpenTorrent: (String) -> Unit,
) {
    item(key = "auto") { AutoDownloadCard(state.rules, vm) }
    if (state.anime.isEmpty() && state.completedTorrents.isEmpty()) {
        item(key = "empty") {
            EmptyState(icon = Icons.Outlined.VideoFile, title = stringResource(R.string.downloads_empty_anime), message = stringResource(R.string.downloads_empty_anime_body))
        }
        return
    }
    state.anime.forEach { group ->
        item(key = "g:" + group.ref.anilistId + ":" + group.ref.malId + ":" + group.title) {
            GroupHeader(group, onOpenDetails = group.ref.anilistId?.let { id -> { onOpenDetails(id) } }, onRemove = { vm.removeTitle(group) })
        }
        items(group.episodes, key = { "e:" + it.id }) { ep -> EpisodeRow(ep, play, vm) }
    }
    if (state.completedTorrents.isNotEmpty()) {
        item(key = "ct-header") { ListGroupHeader(stringResource(R.string.downloads_torrents)) }
        items(state.completedTorrents, key = { "ct:" + it.id }) { t -> TorrentRow(t, onOpenTorrent, vm) }
    }
}

@Composable
private fun GroupHeader(group: TitleGroup, onOpenDetails: (() -> Unit)?, onRemove: () -> Unit) {
    var confirm by remember { mutableStateOf(false) }
    Row(
        Modifier.fillMaxWidth().then(if (onOpenDetails != null) Modifier.clickable(onClick = onOpenDetails) else Modifier).padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaCover(url = group.poster, modifier = Modifier.width(48.dp))
        Column(Modifier.weight(1f)) {
            Text(group.title, style = MaterialTheme.typography.titleMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(
                stringResource(R.string.downloads_group_summary, group.episodes.size, formatBytes(group.bytes)),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        IconButton(onClick = { confirm = true }) { Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.downloads_remove_title)) }
    }
    if (confirm) {
        ConfirmDialog(
            title = stringResource(R.string.downloads_remove_title),
            text = stringResource(R.string.downloads_remove_title_confirm, group.title),
            onConfirm = onRemove,
            onDismiss = { confirm = false },
        )
    }
}

@Composable
private fun EpisodeRow(item: DownloadItem, play: (DownloadItem) -> Unit, vm: DownloadsViewModel) {
    val broken = item.state == DownloadState.BROKEN
    Row(
        Modifier.fillMaxWidth().clickable(enabled = !broken) { play(item) }.padding(start = 76.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Column(Modifier.weight(1f)) {
            Text(stringResource(UiR.string.ui_episode_number, item.episode), style = MaterialTheme.typography.bodyMedium)
            Text(
                listOfNotNull(item.sourceLabel.takeIf { it.isNotBlank() }, (item.totalBytes ?: item.downloadedBytes).takeIf { it > 0 }?.let(::formatBytes)).joinToString(" · "),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (broken) {
            Pill(stringResource(DownloadState.BROKEN.labelRes()), tone = PillTone.ERR)
            IconButton(onClick = { vm.retry(item.id) }) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.downloads_redownload)) }
        } else {
            IconButton(onClick = { play(item) }) { Icon(Icons.Filled.PlayArrow, contentDescription = stringResource(R.string.downloads_play)) }
        }
        IconButton(onClick = { vm.remove(item.id) }) { Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.downloads_delete)) }
    }
}

/** Classic AutomationProfiles: per-title rules over WATCHING, profile, "Simular", "Comprobar ahora". */
@Composable
private fun AutoDownloadCard(rules: List<AutoDownloadRule>, vm: DownloadsViewModel) {
    val koin = getKoin()
    val lists = remember { koin.getOrNull<ListsRepository>() }
    val watchingFlow = remember(lists) { lists?.entries(ListStatus.WATCHING) ?: flowOf(emptyList()) }
    val watching by watchingFlow.collectAsStateWithLifecycle(initialValue = emptyList())
    val suggestions = watching.filter { w -> rules.none { r -> sameRef(r.ref, w.ref) } }
    SectionCard(modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp), title = stringResource(R.string.downloads_auto_title)) {
        Text(stringResource(R.string.downloads_auto_body), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        rules.forEach { rule ->
            Column(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(rule.title, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(
                            stringResource(R.string.downloads_auto_last, rule.lastEpisode),
                            style = MaterialTheme.typography.labelSmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                    TextButton(onClick = { vm.preview(rule.ref) }) { Text(stringResource(R.string.downloads_auto_simulate)) }
                    Switch(checked = rule.enabled, onCheckedChange = { on -> vm.setRule(rule.copy(enabled = on)) })
                }
                ProfilePicker(rule.profile) { p -> vm.setRule(rule.copy(profile = p)) }
            }
        }
        if (suggestions.isNotEmpty()) {
            Text(stringResource(R.string.downloads_auto_suggest), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 8.dp))
            suggestions.take(MAX_SUGGESTIONS).forEach { entry ->
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(entry.title, Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    TextButton(onClick = { vm.addRule(entry.ref, entry.title, entry.poster, DownloadProfile.BALANCED_1080) }) {
                        Text(stringResource(R.string.downloads_auto_add))
                    }
                }
            }
        }
        FilledTonalButton(onClick = vm::checkNow, modifier = Modifier.padding(top = 8.dp)) {
            Icon(Icons.Outlined.Refresh, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(stringResource(R.string.downloads_auto_check), Modifier.padding(start = 6.dp))
        }
    }
}

private const val MAX_SUGGESTIONS = 8

@Composable
internal fun ProfilePicker(selected: DownloadProfile, onSelect: (DownloadProfile) -> Unit) {
    Segmented(
        options = DownloadProfile.entries.map { SegmentedOption(it, stringResource(it.labelRes())) },
        selected = selected,
        onSelect = onSelect,
        small = true,
    )
}

internal fun DownloadProfile.labelRes(): Int = when (this) {
    DownloadProfile.BEST -> R.string.downloads_profile_best
    DownloadProfile.BALANCED_1080 -> R.string.downloads_profile_balanced
    DownloadProfile.SAVER_720 -> R.string.downloads_profile_saver
}

internal fun sameRef(a: MediaRef, b: MediaRef): Boolean =
    (a.anilistId != null && a.anilistId == b.anilistId) || (a.malId != null && a.malId == b.malId)

// ------------------------------------------------------------------ Manga / Novelas

private fun LazyListScope.chaptersTab(list: List<ChapterDownload>, kind: ChapterKind, vm: DownloadsViewModel, onOpen: (ChapterDownload) -> Unit) {
    if (list.isEmpty()) {
        item(key = "empty") {
            EmptyState(
                icon = Icons.Outlined.AutoStories,
                title = stringResource(if (kind == ChapterKind.MANGA) R.string.downloads_empty_manga else R.string.downloads_empty_novels),
                message = stringResource(R.string.downloads_empty_chapters_body),
            )
        }
        return
    }
    list.groupBy { it.mediaId }.forEach { (mediaId, chapters) ->
        val first = chapters.first()
        item(key = "ch:$mediaId") {
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                MediaCover(url = first.poster, modifier = Modifier.width(48.dp))
                Text(first.title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f), maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
        items(chapters.sortedBy { it.chapterNumber ?: Double.MAX_VALUE }, key = { "c:" + it.id }) { c ->
            Row(
                Modifier.fillMaxWidth().clickable(enabled = c.state == DownloadState.COMPLETED) { onOpen(c) }.padding(start = 76.dp, end = 8.dp, top = 4.dp, bottom = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column(Modifier.weight(1f)) {
                    Text(c.chapterLabel, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        listOf(stringResource(c.state.labelRes()), formatBytes(c.bytes)).joinToString(" · "),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (c.state == DownloadState.DOWNLOADING || c.state == DownloadState.QUEUED) {
                        LinearProgressIndicator(progress = { c.progress.toFloat().coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth())
                    }
                }
                if (c.state == DownloadState.FAILED || c.state == DownloadState.BROKEN) {
                    IconButton(onClick = { vm.retryChapter(c) }) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(UiR.string.ui_retry)) }
                }
                IconButton(onClick = { vm.removeChapter(c) }) { Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.downloads_delete)) }
            }
        }
    }
}

// ------------------------------------------------------------------ Historial

private fun LazyListScope.historyTab(state: DownloadsUiState, vm: DownloadsViewModel, play: (DownloadItem) -> Unit, @Suppress("UNUSED_PARAMETER") errorText: (Throwable) -> String) {
    if (state.history.isEmpty()) {
        item(key = "empty") {
            EmptyState(icon = Icons.Outlined.History, title = stringResource(R.string.downloads_empty_history), message = stringResource(R.string.downloads_empty_history_body))
        }
        return
    }
    item(key = "clear") {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = Arrangement.End) {
            TextButton(onClick = vm::clearHistory) { Text(stringResource(R.string.downloads_history_clear)) }
        }
    }
    items(state.history, key = { "h:" + it.id }) { item ->
        val date = remember(item.completedAtMs, item.createdAtMs) {
            java.text.DateFormat.getDateTimeInstance(java.text.DateFormat.MEDIUM, java.text.DateFormat.SHORT).format(java.util.Date(item.completedAtMs ?: item.createdAtMs))
        }
        Row(
            Modifier.fillMaxWidth().clickable(enabled = item.state == DownloadState.COMPLETED) { play(item) }.padding(horizontal = 16.dp, vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MediaCover(url = item.poster, modifier = Modifier.width(40.dp))
            Column(Modifier.weight(1f)) {
                Text(item.title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOf(stringResource(UiR.string.ui_episode_number, item.episode), date).joinToString(" · "),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                itemErrorText(item)?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
            }
            Pill(
                stringResource(item.state.labelRes()),
                tone = when (item.state) {
                    DownloadState.COMPLETED -> PillTone.OK
                    DownloadState.FAILED, DownloadState.BROKEN -> PillTone.ERR
                    else -> PillTone.NEUTRAL
                },
            )
            if (item.state != DownloadState.COMPLETED) {
                IconButton(onClick = { vm.retry(item.id) }) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(UiR.string.ui_retry)) }
            }
        }
    }
}

// ------------------------------------------------------------------ Biblioteca local

private fun LazyListScope.localTab(state: DownloadsUiState, vm: DownloadsViewModel, onPlay: (PlayerArgs) -> Unit) {
    item(key = "local-actions") { LocalActions(vm) }
    if (state.library.isEmpty()) {
        item(key = "empty") {
            EmptyState(icon = Icons.Outlined.FolderOpen, title = stringResource(R.string.downloads_empty_local), message = stringResource(R.string.downloads_empty_local_body))
        }
        return
    }
    items(state.library, key = { "l:" + it.id }) { item -> LocalRow(item, vm, onPlay) }
}

@Composable
private fun LocalActions(vm: DownloadsViewModel) {
    val pickFiles = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris ->
        if (uris.isNotEmpty()) vm.importFiles(uris.map { it.toString() })
    }
    val pickFolder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) vm.scanFolder(uri.toString())
    }
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FilledTonalButton(onClick = { pickFiles.launch(arrayOf("video/*")) }) {
            Icon(Icons.Outlined.VideoFile, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(stringResource(R.string.downloads_local_import), Modifier.padding(start = 6.dp))
        }
        OutlinedButton(onClick = { pickFolder.launch(null) }) {
            Icon(Icons.Outlined.FolderOpen, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(stringResource(R.string.downloads_local_scan), Modifier.padding(start = 6.dp))
        }
    }
}

@Composable
private fun LocalRow(item: LocalLibraryItem, vm: DownloadsViewModel, onPlay: (PlayerArgs) -> Unit) {
    var matching by remember { mutableStateOf(false) }
    var confirm by remember { mutableStateOf(false) }
    val title = item.title ?: item.fileName
    Row(
        Modifier.fillMaxWidth().clickable {
            onPlay(PlayerArgs(media = item.ref ?: MediaRef(), title = title, episode = item.episode ?: 1, localUri = item.uri))
        }.padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaCover(url = item.poster, modifier = Modifier.width(40.dp))
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                listOfNotNull(item.episode?.let { stringResource(UiR.string.ui_episode_number, it) }, item.fileName, formatBytes(item.sizeBytes)).joinToString(" · "),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            if (!item.matched) Pill(stringResource(R.string.downloads_local_unmatched), tone = PillTone.WARN)
        }
        IconButton(onClick = { matching = true }) { Icon(Icons.Outlined.Link, contentDescription = stringResource(R.string.downloads_local_match)) }
        IconButton(onClick = { confirm = true }) { Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.downloads_delete)) }
    }
    if (matching) MatchDialog(item, onDismiss = { matching = false }) { media, ep -> vm.matchLocal(item.id, media, ep) }
    if (confirm) {
        AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.downloads_local_remove)) },
            text = { Text(stringResource(R.string.downloads_local_remove_body, item.fileName)) },
            confirmButton = {
                TextButton(onClick = {
                    vm.removeLocal(item.id, deleteFile = true)
                    confirm = false
                }) { Text(stringResource(R.string.downloads_local_remove_file)) }
            },
            dismissButton = {
                TextButton(onClick = {
                    vm.removeLocal(item.id, deleteFile = false)
                    confirm = false
                }) { Text(stringResource(R.string.downloads_local_remove_entry)) }
            },
        )
    }
}

/** Emparejar / reparar (classic ImportMappingModal): search the title, pick it, set the episode. */
@Composable
private fun MatchDialog(item: LocalLibraryItem, onDismiss: () -> Unit, onMatch: (Media?, Int?) -> Unit) {
    val koin = getKoin()
    val metadata = remember { koin.getOrNull<MetadataRepository>() }
    var query by remember { mutableStateOf(item.title ?: item.fileName.substringBeforeLast('.')) }
    var episode by remember { mutableStateOf(item.episode?.toString().orEmpty()) }
    var results by remember { mutableStateOf<List<Media>>(emptyList()) }
    var picked by remember { mutableStateOf<Media?>(null) }
    var searching by remember { mutableStateOf(false) }
    var failed by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.downloads_local_match)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = query,
                    onValueChange = { query = it },
                    singleLine = true,
                    label = { Text(stringResource(R.string.downloads_local_match_title)) },
                    trailingIcon = {
                        IconButton(onClick = {
                            val repo = metadata
                            if (repo != null) {
                                searching = true
                                failed = false
                                scope.launch {
                                    val r = runCatching { repo.search(MediaKind.ANIME, query).items.take(6) }
                                    results = r.getOrDefault(emptyList())
                                    failed = r.isFailure
                                    searching = false
                                }
                            }
                        }) { Icon(Icons.Outlined.Search, contentDescription = stringResource(R.string.downloads_search)) }
                    },
                )
                if (searching) LinearProgressIndicator(Modifier.fillMaxWidth())
                if (failed) Text(stringResource(R.string.downloads_local_match_error), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                results.forEach { m ->
                    Surface(
                        onClick = { picked = m },
                        color = if (picked == m) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surface,
                        shape = MaterialTheme.shapes.small,
                    ) {
                        Row(Modifier.fillMaxWidth().padding(6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                            MediaCover(url = m.cover, modifier = Modifier.width(32.dp))
                            Text(m.title, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
                OutlinedTextField(
                    value = episode,
                    onValueChange = { v -> episode = v.filter { it.isDigit() }.take(4) },
                    singleLine = true,
                    label = { Text(stringResource(R.string.downloads_local_match_episode)) },
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                onMatch(picked ?: currentMedia(item), episode.toIntOrNull())
                onDismiss()
            }) { Text(stringResource(R.string.downloads_local_match_save)) }
        },
        dismissButton = {
            TextButton(onClick = {
                onMatch(null, null)
                onDismiss()
            }) { Text(stringResource(R.string.downloads_local_match_clear)) }
        },
    )
}

/** Keeps the current match when only the episode changes. */
private fun currentMedia(item: LocalLibraryItem): Media? {
    val ref = item.ref ?: return null
    return Media(
        ref = ref, kind = MediaKind.ANIME, title = item.title ?: item.fileName, titles = emptyMap(), cover = item.poster, banner = null, color = null,
        synopsis = null, genres = emptyList(), episodes = null, status = null, season = null, year = null, score = null, sourceProvider = "local",
    )
}

@Composable
internal fun ConfirmDialog(title: String, text: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = { Text(text) },
        confirmButton = {
            TextButton(onClick = {
                onConfirm()
                onDismiss()
            }) { Text(title) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_cancel)) } },
    )
}
