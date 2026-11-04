package app.tatakai.next.feature.extensions

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import android.text.format.DateUtils
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.FileOpen
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Language
import androidx.compose.material.icons.outlined.Public
import androidx.compose.material.icons.outlined.Storage
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material.icons.outlined.WarningAmber
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.ExtLogLevel
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.ExtensionDetail
import app.tatakai.next.model.ExtensionLogEntry
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.ui.R as UiR
import org.koin.compose.viewmodel.koinViewModel

/**
 * Extensions (prototype `S.extensions`, 07 §1/§4): install a `.kai` with the SAF picker, one card per extension with the compatibility pill
 * and its explanatory banner, permission chips with icons, hash / domains / date, a collapsible log, enable / disable and remove.
 * States: loading indicator after 150 ms while the list is read, empty state, inline install error, per-row detail error with retry.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ExtensionsScreen(
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    viewModel: ExtensionsViewModel = koinViewModel(),
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val snackbar = LocalAppSnackbar.current
    val invalidText = stringResource(R.string.extensions_install_invalid)
    // SAF: the user picks the .kai package; no storage permission needed. Android may not know the .kai type, so allow any and check the name.
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        val name = displayName(context, uri)
        if (name != null && !name.endsWith(".kai", ignoreCase = true)) {
            snackbar.show(invalidText)
        } else {
            viewModel.install(uri.toString())
        }
    }
    var pendingRemove by remember { mutableStateOf<InstalledExtension?>(null) }

    val installedText = (state.message as? InstallMessage.Installed)?.let { stringResource(R.string.extensions_installed, it.name) }
    LaunchedEffect(state.message) {
        if (state.message is InstallMessage.Installed && installedText != null) {
            snackbar.show(installedText)
            viewModel.dismissMessage()
        }
    }

    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.extensions_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
                    }
                },
                actions = {
                    if (state.items.isNotEmpty()) {
                        Pill(
                            pluralStringResource(R.plurals.extensions_installed_count, state.items.size, state.items.size),
                            modifier = Modifier.padding(end = 16.dp),
                            tone = PillTone.NEUTRAL,
                        )
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
            LazyColumn(
                modifier = Modifier.fillMaxSize().widthIn(max = 840.dp),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 32.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                item(key = "intro", contentType = "intro") {
                    Text(
                        stringResource(R.string.extensions_intro),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                item(key = "install", contentType = "install") {
                    InstallPanel(installing = state.installing, onPick = { picker.launch(arrayOf("*/*")) })
                }
                (state.message as? InstallMessage.Failed)?.let { failed ->
                    item(key = "install-error", contentType = "message") {
                        ErrorState(
                            message = stringResource(R.string.extensions_install_failed, failed.reason),
                            retryLabel = stringResource(R.string.extensions_install_choose),
                            onRetry = {
                                viewModel.dismissMessage()
                                picker.launch(arrayOf("*/*"))
                            },
                            compact = true,
                        )
                    }
                }
                when {
                    state.loading -> item(key = "loading", contentType = "state") { DelayedLoading() }
                    state.items.isEmpty() -> item(key = "empty", contentType = "state") {
                        EmptyState(
                            icon = Icons.Outlined.Extension,
                            title = stringResource(R.string.extensions_empty_title),
                            message = stringResource(R.string.extensions_empty_message),
                        )
                    }
                    else -> items(state.items, key = { it.id }, contentType = { "extension" }) { ext ->
                        val enabledText = stringResource(R.string.extensions_now_enabled, ext.name)
                        val disabledText = stringResource(R.string.extensions_now_disabled, ext.name)
                        ExtensionCard(
                            ext = ext,
                            detail = state.details[ext.id] ?: DetailState.Loading,
                            logOpen = ext.id in state.expandedLogs,
                            viewModel = viewModel,
                            onToggle = { on ->
                                viewModel.setEnabled(ext.id, on)
                                snackbar.show(if (on) enabledText else disabledText)
                            },
                            onRemove = { pendingRemove = ext },
                        )
                    }
                }
            }
        }
    }

    pendingRemove?.let { ext ->
        val removedText = stringResource(R.string.extensions_removed, ext.name)
        AlertDialog(
            onDismissRequest = { pendingRemove = null },
            icon = { Icon(Icons.Outlined.Delete, contentDescription = null) },
            title = { Text(stringResource(R.string.extensions_remove_title, ext.name)) },
            text = { Text(stringResource(R.string.extensions_remove_message)) },
            confirmButton = {
                TextButton(onClick = {
                    viewModel.remove(ext.id)
                    snackbar.show(removedText)
                    pendingRemove = null
                }) { Text(stringResource(R.string.extensions_remove_confirm), color = MaterialTheme.colorScheme.error) }
            },
            dismissButton = { TextButton(onClick = { pendingRemove = null }) { Text(stringResource(UiR.string.ui_close)) } },
        )
    }
}

@Composable
private fun DelayedLoading() {
    if (rememberAfterDelay()) LoadingState(Modifier.fillMaxWidth().height(160.dp))
}

@Composable
private fun InstallPanel(installing: Boolean, onPick: () -> Unit) {
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainerLow, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            Icon(Icons.Outlined.FileOpen, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(32.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(stringResource(R.string.extensions_install), style = MaterialTheme.typography.titleSmall)
                Text(
                    stringResource(R.string.extensions_install_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            Button(onClick = onPick, enabled = !installing) {
                Text(stringResource(if (installing) R.string.extensions_installing else R.string.extensions_install_choose))
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ExtensionCard(
    ext: InstalledExtension,
    detail: DetailState,
    logOpen: Boolean,
    viewModel: ExtensionsViewModel,
    onToggle: (Boolean) -> Unit,
    onRemove: () -> Unit,
) {
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            // head: initial, name + compat pill, version · id, switch, remove
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Box(
                    Modifier.size(40.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        ext.name.take(1).uppercase(),
                        style = MaterialTheme.typography.titleMedium,
                        color = MaterialTheme.colorScheme.onPrimaryContainer,
                    )
                }
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                        Text(
                            ext.name,
                            style = MaterialTheme.typography.titleMedium,
                            modifier = Modifier.semantics { heading() },
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                        Pill(stringResource(ext.compat.labelRes()), tone = ext.compat.tone())
                    }
                    Text(
                        "${stringResource(R.string.extensions_version, ext.version)} · ${stringResource(kindRes(ext.type))} · ${ext.id}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                val enabledLabel = stringResource(R.string.extensions_enabled)
                Switch(
                    checked = ext.enabled,
                    onCheckedChange = onToggle,
                    enabled = ext.compat != ExtensionCompat.UNSUPPORTED,
                    modifier = Modifier.semantics { contentDescription = enabledLabel },
                )
                IconButton(onClick = onRemove) {
                    Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.extensions_remove))
                }
            }
            if (ext.description.isNotBlank()) {
                Text(
                    ext.description,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Permissions(ext.permissions)
            CompatBanner(ext)
            Foot(ext, detail, logOpen, onRetry = { viewModel.retryDetail(ext.id) }, onToggleLog = { viewModel.toggleLog(ext.id) })
            if (logOpen) LogPanel(ext.id, viewModel)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Permissions(permissions: List<String>) {
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text(
            stringResource(R.string.extensions_permissions),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            if (permissions.isEmpty()) {
                InfoChip(stringResource(R.string.extensions_no_permissions))
            } else {
                permissions.take(MAX_PERMISSION_CHIPS).forEach { p -> InfoChip(p, icon = permissionIcon(p)) }
                if (permissions.size > MAX_PERMISSION_CHIPS) InfoChip(stringResource(R.string.extensions_more, permissions.size - MAX_PERMISSION_CHIPS))
            }
        }
    }
}

@Composable
private fun CompatBanner(ext: InstalledExtension) {
    val (text, warn) = when (ext.compat) {
        ExtensionCompat.FULL -> return
        ExtensionCompat.PARTIAL -> stringResource(R.string.extensions_compat_partial_hint) to false
        ExtensionCompat.UNSUPPORTED -> stringResource(R.string.extensions_cannot_enable) to true
    }
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = if (warn) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.secondaryContainer,
        contentColor = if (warn) MaterialTheme.colorScheme.onErrorContainer else MaterialTheme.colorScheme.onSecondaryContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(12.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            Icon(if (warn) Icons.Outlined.WarningAmber else Icons.Outlined.Info, contentDescription = null, modifier = Modifier.size(20.dp))
            Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(text, style = MaterialTheme.typography.bodyMedium)
                ext.compatReason?.takeIf { it.isNotBlank() }?.let { reason ->
                    Text(stringResource(R.string.extensions_compat_reason, reason), style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Foot(ext: InstalledExtension, detail: DetailState, logOpen: Boolean, onRetry: () -> Unit, onToggleLog: () -> Unit) {
    when (detail) {
        DetailState.Loading -> if (rememberAfterDelay()) SkeletonBox(Modifier.fillMaxWidth(0.6f).height(16.dp))
        DetailState.Failed -> ErrorState(
            message = stringResource(R.string.extensions_detail_error),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = onRetry,
            compact = true,
        )
        is DetailState.Ready -> detail.detail?.let { FootFacts(it) }
    }
    if (ext.browserDomains.isNotEmpty()) {
        InfoChip(pluralStringResource(R.plurals.extensions_browser, ext.browserDomains.size, ext.browserDomains.size), icon = Icons.Outlined.Public)
    }
    TextButton(onClick = onToggleLog) {
        Icon(Icons.Outlined.Terminal, contentDescription = null, modifier = Modifier.size(18.dp))
        Text(stringResource(if (logOpen) R.string.extensions_hide_logs else R.string.extensions_logs), modifier = Modifier.padding(start = 8.dp))
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun FootFacts(d: ExtensionDetail) {
    val style = MaterialTheme.typography.bodySmall
    val color = MaterialTheme.colorScheme.onSurfaceVariant
    FlowRow(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        d.installedAtMs?.let { at ->
            val rel = DateUtils.getRelativeTimeSpanString(at, System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS).toString()
            Text(stringResource(R.string.extensions_updated, rel), style = style, color = color)
        }
        if (d.hash.isNotEmpty()) Text(stringResource(R.string.extensions_hash, d.hash), style = style, color = color, fontFamily = FontFamily.Monospace)
        Text(pluralStringResource(R.plurals.extensions_domains, d.domains.size, d.domains.size), style = style, color = color)
        if (d.sourceRoutes.isNotEmpty()) {
            val routes = d.sourceRoutes.map { r ->
                when (r) {
                    "torrent" -> stringResource(R.string.extensions_route_torrent)
                    else -> stringResource(R.string.extensions_route_stream)
                }
            }.distinct().joinToString(", ")
            Text(stringResource(R.string.extensions_routes, routes), style = style, color = color)
        }
    }
}

@Composable
private fun LogPanel(extensionId: String, viewModel: ExtensionsViewModel) {
    val flow = remember(extensionId) { viewModel.logs(extensionId) }
    val lines by flow.collectAsStateWithLifecycle(initialValue = emptyList())
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = MaterialTheme.colorScheme.surfaceContainerHighest,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            Modifier.heightIn(max = 260.dp).verticalScroll(rememberScrollState()).padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            if (lines.isEmpty()) {
                Text(
                    stringResource(R.string.extensions_logs_empty),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                lines.forEach { LogLine(it) }
            }
        }
    }
}

@Composable
private fun LogLine(entry: ExtensionLogEntry) {
    val (levelRes, color) = when (entry.level) {
        ExtLogLevel.OK -> R.string.extensions_log_ok to MaterialTheme.colorScheme.primary
        ExtLogLevel.WARN -> R.string.extensions_log_warn to MaterialTheme.colorScheme.tertiary
        ExtLogLevel.ERR -> R.string.extensions_log_err to MaterialTheme.colorScheme.error
    }
    Text(
        stringResource(R.string.extensions_log_line, stringResource(levelRes), entry.op, entry.message),
        style = MaterialTheme.typography.bodySmall,
        fontFamily = FontFamily.Monospace,
        color = if (entry.level == ExtLogLevel.OK) MaterialTheme.colorScheme.onSurface else color,
    )
}

private const val MAX_PERMISSION_CHIPS = 6

private fun permissionIcon(p: String): ImageVector = when {
    p.startsWith("network") -> Icons.Outlined.Language
    p.startsWith("storage") -> Icons.Outlined.Storage
    else -> Icons.Outlined.WarningAmber
}

private fun ExtensionCompat.labelRes(): Int = when (this) {
    ExtensionCompat.FULL -> R.string.extensions_compat_full
    ExtensionCompat.PARTIAL -> R.string.extensions_compat_partial
    ExtensionCompat.UNSUPPORTED -> R.string.extensions_compat_unsupported
}

private fun ExtensionCompat.tone(): PillTone = when (this) {
    ExtensionCompat.FULL -> PillTone.OK
    ExtensionCompat.PARTIAL -> PillTone.WARN
    ExtensionCompat.UNSUPPORTED -> PillTone.ERR
}

private fun kindRes(type: String): Int = when (type) {
    "torrent" -> R.string.extensions_kind_torrent
    "custom" -> R.string.extensions_kind_custom
    else -> R.string.extensions_kind_onlinestream
}

/** Display name of a SAF document, or null when the provider does not say. */
private fun displayName(context: Context, uri: Uri): String? =
    runCatching {
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
            if (c.moveToFirst()) c.getString(0) else null
        }
    }.getOrNull()
