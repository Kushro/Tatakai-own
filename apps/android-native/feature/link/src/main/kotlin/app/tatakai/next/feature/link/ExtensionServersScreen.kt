// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class, ExperimentalLayoutApi::class)

package app.tatakai.next.feature.link

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.CloudOff
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.ContentPaste
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.PhotoCamera
import androidx.compose.material.icons.outlined.QrCode
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.designsystem.theme.TatakaiFonts
import app.tatakai.next.extensionserver.ServerStatus
import org.koin.compose.viewmodel.koinViewModel
import app.tatakai.next.ui.R as UiR

/**
 * "PC extension server" (docs/redesign/13 §3.10, D8): connect this phone to the extension server of Tatakai on a Windows / Linux PC
 * (scan the QR, paste the `tatakai://extensionserver/connect` link, or address + viewer password), manage the linked servers and read
 * what linking implies (advantages, disadvantages, extras, coming soon). A link opened from outside the app asks before pairing.
 */
@Composable
fun ExtensionServersScreen(onBack: () -> Unit, modifier: Modifier = Modifier) {
    val viewModel: ExtensionServersViewModel = koinViewModel()
    val state by viewModel.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val context = LocalContext.current
    LaunchedEffect(viewModel) {
        viewModel.events.collect { event ->
            when (event) {
                is ExtensionServersEvent.Connected -> snackbar.show(context.getString(R.string.extserver_connected, event.name))
            }
        }
    }
    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.extserver_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
                    }
                },
                actions = {
                    IconButton(onClick = viewModel::refresh) {
                        Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.extserver_refresh))
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
            Column(
                Modifier
                    .fillMaxWidth()
                    .widthIn(max = 840.dp)
                    .verticalScroll(rememberScrollState())
                    .padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                Text(
                    stringResource(R.string.extserver_intro),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                state.error?.let { ErrorBanner(errorText(it), onDismiss = viewModel::clearError) }
                ConnectPanel(
                    connecting = state.connecting,
                    isPlausible = viewModel::isPlausibleLink,
                    onLink = viewModel::connectWithText,
                    onManual = viewModel::connectManual,
                )
                ServersPanel(
                    state = state,
                    onEnabled = viewModel::setEnabled,
                    onRemove = viewModel::remove,
                )
                InfoPanel()
            }
        }
    }
    state.pendingLink?.let { link ->
        AlertDialog(
            onDismissRequest = viewModel::dismissPending,
            icon = { Icon(Icons.Outlined.Shield, contentDescription = null) },
            title = { Text(stringResource(R.string.extserver_confirm_title)) },
            text = { Text(stringResource(R.string.extserver_confirm_body, link.server)) },
            confirmButton = { TextButton(onClick = viewModel::confirmPending) { Text(stringResource(R.string.extserver_connect)) } },
            dismissButton = { TextButton(onClick = viewModel::dismissPending) { Text(stringResource(UiR.string.ui_cancel)) } },
        )
    }
}

// ---------------------------------------------------------------- connect

@Composable
private fun ConnectPanel(
    connecting: Boolean,
    isPlausible: (String) -> Boolean,
    onLink: (String) -> Unit,
    onManual: (String, String) -> Unit,
) {
    val context = LocalContext.current
    var scanning by rememberSaveable { mutableStateOf(false) }
    var denied by rememberSaveable { mutableStateOf(false) }
    var pasting by rememberSaveable { mutableStateOf(false) }
    var url by rememberSaveable { mutableStateOf("") }
    // Not saveable on purpose: the password must not end up in the saved instance state.
    var password by remember { mutableStateOf("") }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        denied = !granted
        scanning = granted
    }

    Panel(icon = Icons.Outlined.QrCode, title = stringResource(R.string.extserver_pair_title)) {
        Hint(stringResource(R.string.extserver_pair_hint))
        if (scanning) {
            QrScanner(
                onResult = {
                    scanning = false
                    onLink(it)
                },
                modifier = Modifier.fillMaxWidth().aspectRatio(1f).clip(MaterialTheme.shapes.large),
            )
            TextButton(onClick = { scanning = false }) { Text(stringResource(R.string.extserver_scan_cancel)) }
        } else {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = {
                        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                            PackageManager.PERMISSION_GRANTED
                        if (granted) scanning = true else permission.launch(Manifest.permission.CAMERA)
                    },
                    enabled = !connecting,
                ) {
                    if (connecting) {
                        LoadingIndicator(Modifier.size(20.dp))
                    } else {
                        Icon(Icons.Outlined.PhotoCamera, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                    }
                    Text(
                        stringResource(if (connecting) R.string.extserver_connecting else R.string.extserver_scan),
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
                FilledTonalButton(onClick = { pasting = true }, enabled = !connecting) {
                    Icon(Icons.Outlined.ContentPaste, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                    Text(stringResource(R.string.extserver_paste_link), modifier = Modifier.padding(start = 8.dp))
                }
            }
        }
        if (denied) {
            Text(
                stringResource(R.string.extserver_camera_denied),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
        HorizontalDivider()
        Text(stringResource(R.string.extserver_manual_title), style = MaterialTheme.typography.labelLarge)
        OutlinedTextField(
            value = url,
            onValueChange = { url = it },
            label = { Text(stringResource(R.string.extserver_url_label)) },
            placeholder = { Text(stringResource(R.string.extserver_url_placeholder)) },
            singleLine = true,
            textStyle = TextStyle(fontFamily = TatakaiFonts.jetBrainsMono),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text(stringResource(R.string.extserver_password_label)) },
            supportingText = { Text(stringResource(R.string.extserver_password_hint)) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
            modifier = Modifier.fillMaxWidth(),
        )
        Button(
            onClick = {
                onManual(url, password)
                password = ""
            },
            enabled = !connecting && url.isNotBlank(),
        ) { Text(stringResource(if (connecting) R.string.extserver_connecting else R.string.extserver_connect)) }
        Hint(stringResource(R.string.extserver_lan_only_http))
        Hint(stringResource(R.string.extserver_away_from_home))
    }
    if (pasting) {
        PasteLinkDialog(
            isPlausible = isPlausible,
            onDismiss = { pasting = false },
            onConnect = {
                pasting = false
                onLink(it)
            },
        )
    }
}

@Composable
private fun PasteLinkDialog(isPlausible: (String) -> Boolean, onDismiss: () -> Unit, onConnect: (String) -> Unit) {
    var text by rememberSaveable { mutableStateOf("") }
    var showError by rememberSaveable { mutableStateOf(false) }
    val invalid = showError && !isPlausible(text)
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.QrCode, contentDescription = null) },
        title = { Text(stringResource(R.string.extserver_paste_link)) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = {
                    text = it
                    showError = false
                },
                label = { Text(stringResource(R.string.extserver_link_label)) },
                placeholder = { Text(stringResource(R.string.extserver_link_placeholder)) },
                isError = invalid,
                supportingText = if (invalid) {
                    { Text(stringResource(R.string.extserver_err_invalid_link)) }
                } else {
                    null
                },
                textStyle = TextStyle(fontFamily = TatakaiFonts.jetBrainsMono),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Done),
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(
                onClick = { if (isPlausible(text)) onConnect(text.trim()) else showError = true },
                enabled = text.isNotBlank(),
            ) { Text(stringResource(R.string.extserver_connect)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_cancel)) } },
    )
}

// ---------------------------------------------------------------- servers

@Composable
private fun ServersPanel(state: ExtensionServersUiState, onEnabled: (String, Boolean) -> Unit, onRemove: (String) -> Unit) {
    Panel(icon = Icons.Outlined.Computer, title = stringResource(R.string.extserver_servers_title)) {
        when {
            !state.loaded -> if (rememberAfterDelay(150)) {
                SkeletonBox(Modifier.fillMaxWidth().aspectRatio(6f))
            }
            state.servers.isEmpty() -> EmptyState(
                icon = Icons.Outlined.Extension,
                title = stringResource(R.string.extserver_none),
                message = stringResource(R.string.extserver_none_body),
            )
            else -> {
                state.servers.forEachIndexed { i, row ->
                    if (i > 0) HorizontalDivider()
                    ServerItem(row, onEnabled = { onEnabled(row.connection.id, it) }, onRemove = { onRemove(row.connection.id) })
                }
                if (state.servers.any { it.connection.enabled && it.status == ServerStatus.Offline }) {
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Outlined.CloudOff, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                        Hint(stringResource(R.string.extserver_offline_fallback))
                    }
                }
            }
        }
    }
}

@Composable
private fun ServerItem(row: ServerRow, onEnabled: (Boolean) -> Unit, onRemove: () -> Unit) {
    var confirming by rememberSaveable { mutableStateOf(false) }
    val conn = row.connection
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(conn.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    conn.url,
                    style = MaterialTheme.typography.bodySmall.copy(fontFamily = TatakaiFonts.jetBrainsMono),
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Switch(checked = conn.enabled, onCheckedChange = onEnabled)
            IconButton(onClick = { confirming = true }) {
                Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.extserver_remove))
            }
        }
        StatusPill(enabled = conn.enabled, status = row.status)
        if (conn.enabled && row.status == ServerStatus.Unauthorized) {
            Text(
                stringResource(R.string.extserver_unauthorized),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
    if (confirming) {
        AlertDialog(
            onDismissRequest = { confirming = false },
            icon = { Icon(Icons.Outlined.Delete, contentDescription = null) },
            title = { Text(stringResource(R.string.extserver_remove_title)) },
            text = { Text(stringResource(R.string.extserver_remove_body, conn.name)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirming = false
                        onRemove()
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) { Text(stringResource(R.string.extserver_remove)) }
            },
            dismissButton = { TextButton(onClick = { confirming = false }) { Text(stringResource(UiR.string.ui_cancel)) } },
        )
    }
}

@Composable
private fun StatusPill(enabled: Boolean, status: ServerStatus) {
    if (!enabled) {
        Pill(stringResource(R.string.extserver_disabled))
        return
    }
    when (status) {
        is ServerStatus.Online -> Pill(
            status.sourceCount?.let { stringResource(R.string.extserver_online_sources, it) } ?: stringResource(R.string.extserver_online),
            tone = PillTone.OK,
        )
        ServerStatus.Offline -> Pill(stringResource(R.string.extserver_offline), tone = PillTone.ERR)
        ServerStatus.Unauthorized -> Pill(stringResource(R.string.extserver_revoked), tone = PillTone.WARN)
        ServerStatus.Checking -> Pill(stringResource(R.string.extserver_checking))
        ServerStatus.Unknown -> Pill(stringResource(R.string.extserver_unknown))
    }
}

// ---------------------------------------------------------------- D8 information

@Composable
private fun InfoPanel() {
    Panel(icon = Icons.Outlined.Info, title = stringResource(R.string.extserver_benefits_title)) {
        InfoList(
            stringResource(R.string.extserver_pros_title),
            listOf(
                R.string.extserver_pro_other_ecosystems,
                R.string.extserver_pro_install_once,
                R.string.extserver_pro_pc_scrapes,
                R.string.extserver_pro_shared_cache,
                R.string.extserver_pro_battery,
            ),
        )
        InfoList(
            stringResource(R.string.extserver_cons_title),
            listOf(
                R.string.extserver_con_pc_on,
                R.string.extserver_con_tunnel,
                R.string.extserver_con_video_proxy,
                R.string.extserver_con_lan_port,
                R.string.extserver_con_latency,
            ),
        )
        InfoList(
            stringResource(R.string.extserver_extras_title),
            listOf(
                R.string.extserver_extra_qr,
                R.string.extserver_extra_revocable,
                R.string.extserver_extra_fallback,
                R.string.extserver_extra_viewer_password,
            ),
        )
        Surface(
            color = MaterialTheme.colorScheme.secondaryContainer,
            contentColor = MaterialTheme.colorScheme.onSecondaryContainer,
            shape = MaterialTheme.shapes.medium,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(stringResource(R.string.extserver_coming_soon_title), style = MaterialTheme.typography.labelLarge)
                Text(stringResource(R.string.extserver_coming_soon_desktop_next), style = MaterialTheme.typography.bodySmall)
            }
        }
    }
}

@Composable
private fun InfoList(title: String, items: List<Int>) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(title, style = MaterialTheme.typography.labelLarge, modifier = Modifier.semantics { heading() })
        items.forEach { res ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("•", style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.primary)
                Text(stringResource(res), style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

// ---------------------------------------------------------------- building blocks

@Composable
private fun errorText(error: ExtServerError): String = when (error) {
    ExtServerError.InvalidLink -> stringResource(R.string.extserver_err_invalid_link)
    is ExtServerError.InsecureHost -> error.host?.takeIf { it.isNotBlank() }
        ?.let { stringResource(R.string.extserver_err_insecure_host, it) }
        ?: stringResource(R.string.extserver_err_insecure_generic)
    ExtServerError.InvalidUrl -> stringResource(R.string.extserver_err_invalid_url)
    ExtServerError.ExpiredLink -> stringResource(R.string.extserver_err_expired_link)
    ExtServerError.Unreachable -> stringResource(R.string.extserver_err_unreachable)
    ExtServerError.WrongPassword -> stringResource(R.string.extserver_err_wrong_password)
    ExtServerError.PairingRequired -> stringResource(R.string.extserver_err_pairing_required)
    ExtServerError.Incompatible -> stringResource(R.string.extserver_err_incompatible)
    ExtServerError.SetupRequired -> stringResource(R.string.extserver_err_setup_required)
    ExtServerError.Throttled -> stringResource(R.string.extserver_err_throttled)
    is ExtServerError.Other -> stringResource(R.string.extserver_err_generic, error.code)
}

@Composable
private fun ErrorBanner(text: String, onDismiss: () -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.errorContainer,
        contentColor = MaterialTheme.colorScheme.onErrorContainer,
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(start = 16.dp, top = 8.dp, bottom = 8.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_cancel)) }
        }
    }
}

/** Prototype `.panel` with a titled header (same look as the Linked device screen). */
@Composable
private fun Panel(icon: ImageVector, title: String, content: @Composable ColumnScope.() -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        shape = MaterialTheme.shapes.large,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                Text(
                    title,
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.semantics { heading() },
                )
            }
            content()
        }
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}
