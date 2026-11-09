// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CloudDownload
import androidx.compose.material.icons.outlined.FolderOpen
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RangeSlider
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.documentfile.provider.DocumentFile
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.DownloadSettings
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.formatBytes
import kotlinx.coroutines.launch
import org.koin.compose.getKoin
import kotlin.math.roundToInt

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r; checklist downloads.season-modal,
 * details.download-season-mount): mounted by `WaveBSlots` of Details under the episode list. "Descargar temporada" (`download_season` of
 * core/ui) opens an `AdaptiveSheet` with the episode range, audio language and server (`DownloadSettings.preferredServer /
 * preferredAudioLang` as defaults), how many are already downloaded (`DownloadRepository.downloadedEpisodes`) and calls `enqueueSeason`.
 * Only anime with a known episode count (AniList `episodes`, or the episode list of the provider).
 */
@Composable
fun DownloadSeasonSlot(media: Media, modifier: Modifier = Modifier) {
    if (media.kind != MediaKind.ANIME) return
    val koin = getKoin()
    val downloads = remember { koin.getOrNull<DownloadRepository>() } ?: return
    val metadata = remember { koin.getOrNull<MetadataRepository>() }
    val settingsRepo = remember { koin.getOrNull<SettingsRepository>() }
    var total by remember(media.ref) { mutableStateOf(media.episodes ?: 0) }
    LaunchedEffect(media.ref) {
        if (total <= 0 && metadata != null) total = runCatching { metadata.episodes(media.ref).maxOfOrNull { it.number } ?: 0 }.getOrDefault(0)
    }
    val doneFlow = remember(media.ref) { downloads.downloadedEpisodes(media.ref) }
    val done by doneFlow.collectAsStateWithLifecycle(initialValue = emptyMap())
    var open by remember { mutableStateOf(false) }
    if (total <= 0) return
    Row(modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        FilledTonalButton(onClick = { open = true }) {
            Icon(Icons.Outlined.CloudDownload, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(stringResource(UiR.string.download_season), Modifier.padding(start = 6.dp))
        }
        if (done.isNotEmpty()) {
            Text(
                stringResource(R.string.downloads_season_have, done.size, total),
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 12.dp),
            )
        }
    }
    if (open) {
        val defaults = settingsRepo?.settings?.value?.downloads ?: DownloadSettings()
        SeasonSheet(media, total, done.keys, defaults, downloads, settingsRepo, onDismiss = { open = false })
    }
}

@Composable
private fun SeasonSheet(
    media: Media,
    total: Int,
    have: Set<Int>,
    defaults: DownloadSettings,
    downloads: DownloadRepository,
    settingsRepo: SettingsRepository?,
    onDismiss: () -> Unit,
) {
    var range by remember { mutableStateOf(1f..total.toFloat()) }
    var server by remember { mutableStateOf(defaults.preferredServer.orEmpty()) }
    var lang by remember { mutableStateOf(defaults.preferredAudioLang.orEmpty()) }
    var keep by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val snackbar = LocalAppSnackbar.current
    val queuedMsg = stringResource(R.string.downloads_season_queued)
    val errorText = rememberDownloadErrorText()
    val first = range.start.roundToInt().coerceIn(1, total)
    val last = range.endInclusive.roundToInt().coerceIn(first, total)
    val wanted = (first..last).toList()
    val already = wanted.count { it in have }
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(UiR.string.download_season), style = MaterialTheme.typography.titleLarge)
            Text(media.title, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(stringResource(R.string.downloads_season_range, first, last), style = MaterialTheme.typography.titleSmall)
            if (total > 1) {
                RangeSlider(value = range, onValueChange = { range = it }, valueRange = 1f..total.toFloat(), steps = (total - 2).coerceAtLeast(0))
            }
            Text(
                stringResource(R.string.downloads_season_already, already, wanted.size),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            OutlinedTextField(
                value = lang,
                onValueChange = { lang = it.take(8) },
                singleLine = true,
                label = { Text(stringResource(R.string.downloads_pref_audio)) },
                supportingText = { Text(stringResource(R.string.downloads_pref_audio_hint)) },
                modifier = Modifier.fillMaxWidth(),
            )
            OutlinedTextField(
                value = server,
                onValueChange = { server = it.take(40) },
                singleLine = true,
                label = { Text(stringResource(R.string.downloads_pref_server)) },
                supportingText = { Text(stringResource(R.string.downloads_pref_server_hint)) },
                modifier = Modifier.fillMaxWidth(),
            )
            if (settingsRepo != null) {
                Row(Modifier.fillMaxWidth().clickable { keep = !keep }) {
                    Text(stringResource(R.string.downloads_season_remember), Modifier.weight(1f).padding(top = 12.dp))
                    Switch(checked = keep, onCheckedChange = { keep = it })
                }
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp, androidx.compose.ui.Alignment.End)) {
                TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_cancel)) }
                Button(
                    enabled = !busy && already < wanted.size,
                    onClick = {
                        busy = true
                        scope.launch {
                            val s = server.trim().takeIf { it.isNotEmpty() }
                            val l = lang.trim().lowercase().takeIf { it.isNotEmpty() }
                            if (keep) settingsRepo?.update { it.copy(downloads = it.downloads.copy(preferredServer = s, preferredAudioLang = l).sanitized()) }
                            downloads.enqueueSeason(media, wanted, s, l).fold(
                                onSuccess = { snackbar.show(queuedMsg.format(it.size)) },
                                onFailure = { snackbar.show(errorText(it)) },
                            )
                            busy = false
                            onDismiss()
                        }
                    },
                ) { Text(stringResource(R.string.downloads_season_start, wanted.size - already)) }
            }
        }
    }
}

/**
 * Settings › Descargas (`SettingsSection.DOWNLOADS`, mounted by the contract in SettingsScreen; checklist downloads.location / .prefs /
 * .storage-limit / .auto.* / .android.auto-evict): folder (SAF tree picker, `DownloadSettings.directoryTreeUri`), max concurrent, Wi-Fi
 * only, storage limit + usage (`DownloadRepository.storageUsedBytes`), auto-check + profile, auto-evict, preferred server / audio.
 */
@Composable
fun DownloadsSettingsSection(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit, modifier: Modifier = Modifier) {
    val d = settings.downloads
    val context = LocalContext.current
    val koin = getKoin()
    val downloads = remember { koin.getOrNull<DownloadRepository>() }
    var used by remember { mutableStateOf<Long?>(null) }
    LaunchedEffect(downloads) { used = downloads?.let { runCatching { it.storageUsedBytes() }.getOrNull() } }
    fun set(transform: (DownloadSettings) -> DownloadSettings) = onUpdate { it.copy(downloads = transform(it.downloads).sanitized()) }
    val pickFolder = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri: Uri? ->
        if (uri != null) {
            runCatching {
                context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            }
            set { it.copy(directoryTreeUri = uri.toString()) }
        }
    }
    val folderLabel = remember(d.directoryTreeUri) {
        d.directoryTreeUri?.let { runCatching { DocumentFile.fromTreeUri(context, Uri.parse(it))?.name }.getOrNull() ?: it }
            ?: (context.getExternalFilesDir("downloads")?.absolutePath ?: "")
    }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        ListItem(
            headlineContent = { Text(stringResource(R.string.downloads_settings_folder)) },
            supportingContent = {
                Text(
                    if (d.directoryTreeUri == null) stringResource(R.string.downloads_settings_folder_app, folderLabel) else folderLabel,
                )
            },
        )
        Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = { pickFolder.launch(null) }) {
                Icon(Icons.Outlined.FolderOpen, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.downloads_settings_folder_change), Modifier.padding(start = 6.dp))
            }
            if (d.directoryTreeUri != null) {
                TextButton(onClick = { set { it.copy(directoryTreeUri = null) } }) { Text(stringResource(R.string.downloads_settings_folder_reset)) }
            }
        }
        SettingLabel(stringResource(R.string.downloads_settings_concurrent), stringResource(R.string.downloads_settings_concurrent_hint))
        Segmented(
            options = (1..DownloadSettings.MAX_CONCURRENT).map { SegmentedOption(it, it.toString()) },
            selected = d.maxConcurrent,
            onSelect = { n -> set { it.copy(maxConcurrent = n) } },
            small = true,
            modifier = Modifier.padding(horizontal = 16.dp),
        )
        SwitchRow(stringResource(R.string.downloads_settings_wifi), stringResource(R.string.downloads_settings_wifi_hint), d.wifiOnly) { on -> set { it.copy(wifiOnly = on) } }
        SettingLabel(
            stringResource(R.string.downloads_settings_limit),
            used?.let { stringResource(R.string.downloads_settings_limit_used, formatBytes(it)) } ?: stringResource(R.string.downloads_settings_limit_hint),
        )
        Segmented(
            options = STORAGE_STEPS.map { g -> SegmentedOption(g, if (g == 0) stringResource(R.string.downloads_settings_limit_none) else stringResource(R.string.downloads_settings_limit_gib, g)) },
            selected = STORAGE_STEPS.minByOrNull { kotlin.math.abs(it - d.storageLimitGib) } ?: 0,
            onSelect = { g -> set { it.copy(storageLimitGib = g) } },
            small = true,
            modifier = Modifier.padding(horizontal = 16.dp),
        )
        SwitchRow(stringResource(R.string.downloads_settings_auto), stringResource(R.string.downloads_settings_auto_hint), d.autoCheck) { on -> set { it.copy(autoCheck = on) } }
        SettingLabel(stringResource(R.string.downloads_settings_profile), stringResource(R.string.downloads_settings_profile_hint))
        Row(Modifier.padding(horizontal = 16.dp)) { ProfilePicker(d.autoProfile) { p -> set { it.copy(autoProfile = p) } } }
        SwitchRow(stringResource(R.string.downloads_settings_evict), stringResource(R.string.downloads_settings_evict_hint), d.autoEvictWatched) { on ->
            set { it.copy(autoEvictWatched = on) }
        }
        var server by remember(d.preferredServer) { mutableStateOf(d.preferredServer.orEmpty()) }
        var lang by remember(d.preferredAudioLang) { mutableStateOf(d.preferredAudioLang.orEmpty()) }
        OutlinedTextField(
            value = server,
            onValueChange = { v ->
                server = v.take(40)
                set { it.copy(preferredServer = server) }
            },
            singleLine = true,
            label = { Text(stringResource(R.string.downloads_pref_server)) },
            supportingText = { Text(stringResource(R.string.downloads_pref_server_hint)) },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        )
        OutlinedTextField(
            value = lang,
            onValueChange = { v ->
                lang = v.take(8)
                set { it.copy(preferredAudioLang = lang) }
            },
            singleLine = true,
            label = { Text(stringResource(R.string.downloads_pref_audio)) },
            supportingText = { Text(stringResource(R.string.downloads_pref_audio_hint)) },
            modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp),
        )
    }
}

private val STORAGE_STEPS = listOf(0, 5, 10, 25, 50, 100)

@Composable
private fun SettingLabel(title: String, hint: String) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Text(title, style = MaterialTheme.typography.bodyLarge)
        Text(hint, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun SwitchRow(title: String, hint: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(hint) },
        trailingContent = { Switch(checked = checked, onCheckedChange = onChange) },
        modifier = Modifier.clickable { onChange(!checked) },
    )
}
