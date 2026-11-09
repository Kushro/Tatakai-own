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
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.SkipNext
import androidx.compose.material.icons.filled.SkipPrevious
import androidx.compose.material.icons.outlined.Cast
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.ContentPaste
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.Memory
import androidx.compose.material.icons.outlined.PhoneAndroid
import androidx.compose.material.icons.outlined.PhotoCamera
import androidx.compose.material.icons.outlined.QrCode
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberUpdatedState
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LinkStatusAvatar
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.designsystem.theme.TatakaiFonts
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkProfile
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.format.relativeTime
import app.tatakai.next.ui.state.userMessage
import org.koin.compose.viewmodel.koinViewModel
import app.tatakai.next.ui.R as UiR

// android wave contract (docs/redesign/12 §3.13). Owner: worker b. Name and signature FIXED.

/**
 * "Dispositivo enlazado" (prototype android.js `S.linked`): unpaired -> EmptyState + scan QR (CameraX + zxing, QrScanner.kt) / paste
 * code; paired -> status card (connected / unreachable with error + retry), PC capabilities, enhance profile Segmented
 * (LinkRepository.setPreferredProfile), remote control placeholder ("M1", disabled buttons), forget dialog. Replaces LinkScreen for
 * Route.Link and Route.Linked.
 *
 * The "enhance a stream" card of the former LinkScreen is kept below the remote control (it is how [onPlay] starts a PC session).
 */
@Composable
fun LinkedDeviceScreen(
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
    /** docs/redesign/13 §3.10: opens the PC extension server screen; null hides its entry card. */
    onOpenExtensionServers: (() -> Unit)? = null,
) {
    val viewModel: LinkViewModel = koinViewModel()
    val state by viewModel.state.collectAsStateWithLifecycle()
    val currentOnPlay by rememberUpdatedState(onPlay)
    LaunchedEffect(viewModel) {
        viewModel.events.collect { event ->
            when (event) {
                is LinkEvent.NavigateToPlayer -> currentOnPlay(event.args)
            }
        }
    }
    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.linked_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
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
                    stringResource(R.string.linked_intro),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                if (onOpenExtensionServers != null) ExtensionServerCard(onOpen = onOpenExtensionServers)
                when (val link = state.link) {
                    LinkState.Unpaired, LinkState.Pairing -> UnpairedContent(
                        pairing = state.pairing || link == LinkState.Pairing,
                        error = state.pairError?.userMessage(),
                        isValid = viewModel::isValidPairingText,
                        onPair = viewModel::pair,
                    )
                    is LinkState.Failed -> {
                        ErrorState(
                            message = link.message,
                            retryLabel = stringResource(UiR.string.ui_retry),
                            onRetry = viewModel::retryCapabilities,
                            title = stringResource(R.string.linked_problem),
                        )
                        ForgetButton(onForget = viewModel::forget)
                    }
                    is LinkState.Paired -> PairedContent(
                        state = state,
                        paired = link,
                        onRetry = viewModel::retryCapabilities,
                        onSelectProfile = viewModel::selectProfile,
                        onForget = viewModel::forget,
                        onStart = viewModel::startSession,
                    )
                }
            }
        }
    }
}

// ---------------------------------------------------------------- unpaired

@Composable
private fun UnpairedContent(pairing: Boolean, error: String?, isValid: (String) -> Boolean, onPair: (String) -> Unit) {
    val context = LocalContext.current
    var scanning by rememberSaveable { mutableStateOf(false) }
    var denied by rememberSaveable { mutableStateOf(false) }
    var pasting by rememberSaveable { mutableStateOf(false) }
    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        denied = !granted
        scanning = granted
    }

    EmptyState(
        icon = Icons.Outlined.Computer,
        title = stringResource(R.string.linked_none),
        message = stringResource(R.string.linked_none_body),
    )
    Panel(icon = Icons.Outlined.QrCode, title = stringResource(R.string.linked_pair_title)) {
        if (scanning) {
            QrScanner(
                onResult = {
                    scanning = false
                    onPair(it)
                },
                modifier = Modifier.fillMaxWidth().aspectRatio(1f).clip(MaterialTheme.shapes.large),
            )
            TextButton(onClick = { scanning = false }) { Text(stringResource(R.string.linked_scan_cancel)) }
        } else {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(
                    onClick = {
                        val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                            PackageManager.PERMISSION_GRANTED
                        if (granted) scanning = true else permission.launch(Manifest.permission.CAMERA)
                    },
                    enabled = !pairing,
                ) {
                    if (pairing) {
                        LoadingIndicator(Modifier.size(20.dp))
                    } else {
                        Icon(Icons.Outlined.PhotoCamera, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                    }
                    Text(
                        stringResource(if (pairing) R.string.linked_pairing else R.string.linked_scan),
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
                FilledTonalButton(onClick = { pasting = true }, enabled = !pairing) {
                    Icon(Icons.Outlined.ContentPaste, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                    Text(stringResource(R.string.linked_paste), modifier = Modifier.padding(start = 8.dp))
                }
            }
        }
        if (denied) {
            Text(
                stringResource(R.string.linked_camera_denied),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
        if (error != null) {
            Text(error, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.error)
        }
        Hint(stringResource(R.string.linked_camera))
        Hint(stringResource(R.string.linked_discovery_m1))
    }
    if (pasting) {
        PasteCodeDialog(
            isValid = isValid,
            onDismiss = { pasting = false },
            onPair = {
                pasting = false
                onPair(it)
            },
        )
    }
}

@Composable
private fun PasteCodeDialog(isValid: (String) -> Boolean, onDismiss: () -> Unit, onPair: (String) -> Unit) {
    var code by rememberSaveable { mutableStateOf("") }
    var showError by rememberSaveable { mutableStateOf(false) }
    val invalid = showError && !isValid(code)
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.QrCode, contentDescription = null) },
        title = { Text(stringResource(R.string.linked_paste)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.linked_paste_hint), style = MaterialTheme.typography.bodyMedium)
                OutlinedTextField(
                    value = code,
                    onValueChange = {
                        code = it
                        showError = false
                    },
                    label = { Text(stringResource(R.string.linked_code_label)) },
                    singleLine = true,
                    isError = invalid,
                    supportingText = if (invalid) {
                        { Text(stringResource(R.string.linked_paste_invalid)) }
                    } else {
                        null
                    },
                    textStyle = TextStyle(fontFamily = TatakaiFonts.jetBrainsMono),
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(
                onClick = { if (isValid(code)) onPair(code.trim()) else showError = true },
                enabled = code.isNotBlank(),
            ) { Text(stringResource(R.string.linked_pair)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_cancel)) } },
    )
}

// ---------------------------------------------------------------- paired

@Composable
private fun PairedContent(
    state: LinkUiState,
    paired: LinkState.Paired,
    onRetry: () -> Unit,
    onSelectProfile: (String?) -> Unit,
    onForget: () -> Unit,
    onStart: (String) -> Unit,
) {
    val snackbar = LocalAppSnackbar.current
    val m1 = stringResource(UiR.string.ui_m1)
    StatusCard(state, paired, onRetry = onRetry, onContinue = { snackbar.show(m1) })
    CapabilitiesPanel(state, onRetry = onRetry, onSelectProfile = onSelectProfile)
    RemotePanel(onForget = onForget)
    EnhancePanel(state, onStart = onStart)
}

@Composable
private fun StatusCard(state: LinkUiState, paired: LinkState.Paired, onRetry: () -> Unit, onContinue: () -> Unit) {
    val online = paired.reachable
    val device = state.device
    Surface(
        color = if (online) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.errorContainer,
        contentColor = if (online) MaterialTheme.colorScheme.onPrimaryContainer else MaterialTheme.colorScheme.onErrorContainer,
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                LinkStatusAvatar(connected = online, icon = Icons.Outlined.Computer)
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(
                        stringResource(if (online) R.string.linked_connected else R.string.linked_unreachable),
                        style = MaterialTheme.typography.titleMedium,
                    )
                    Text(
                        "${device?.serverName ?: paired.pc.serverName} · https://${paired.pc.host}:${paired.pc.port}",
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    device?.fingerprint?.let {
                        Text(
                            stringResource(R.string.linked_fingerprint, it),
                            style = MaterialTheme.typography.bodySmall.copy(fontFamily = TatakaiFonts.jetBrainsMono),
                        )
                    }
                    device?.lastSeenMs?.let {
                        Text(stringResource(R.string.linked_last_seen, relativeTime(it)), style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
            if (!online) {
                Text(stringResource(R.string.linked_unreachable_hint), style = MaterialTheme.typography.bodySmall)
            }
            Row(horizontalArrangement = Arrangement.End, modifier = Modifier.fillMaxWidth()) {
                if (online) {
                    FilledTonalButton(onClick = onContinue) {
                        Icon(Icons.Outlined.Cast, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                        Text(stringResource(R.string.linked_continue), modifier = Modifier.padding(start = 8.dp))
                    }
                } else {
                    FilledTonalButton(onClick = onRetry) {
                        Icon(Icons.Outlined.Refresh, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                        Text(stringResource(UiR.string.ui_retry), modifier = Modifier.padding(start = 8.dp))
                    }
                }
            }
        }
    }
}

@Composable
private fun CapabilitiesPanel(state: LinkUiState, onRetry: () -> Unit, onSelectProfile: (String?) -> Unit) {
    Panel(icon = Icons.Outlined.Memory, title = stringResource(R.string.linked_caps)) {
        when (val caps = state.capabilities) {
            null, LoadState.Loading -> if (rememberAfterDelay(150)) {
                SkeletonBox(Modifier.fillMaxWidth().aspectRatio(5f))
                SkeletonBox(Modifier.fillMaxWidth(0.6f).aspectRatio(8f))
            }
            is LoadState.Err -> ErrorState(
                message = caps.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = onRetry,
                compact = true,
            )
            is LoadState.Ok -> CapabilitiesBody(caps.value, state.selectedProfile, onSelectProfile)
        }
    }
}

@Composable
private fun CapabilitiesBody(caps: LinkCapabilities, selected: String?, onSelect: (String?) -> Unit) {
    Cap(stringResource(R.string.linked_caps_server), "${caps.serverName} · ${caps.serverVersion}")
    Cap(
        stringResource(R.string.linked_caps_frame_gen),
        stringResource(if (caps.clientFrameGenerationAllowed) R.string.linked_yes else R.string.linked_no),
    )
    // Prototype: profiles measured as not sustainable are hidden from the segmented.
    val usable = caps.profiles.filter { it.measuredSustain != "false" }
    if (usable.isEmpty()) {
        Hint(stringResource(R.string.linked_profiles_none))
        return
    }
    Text(stringResource(R.string.linked_profile), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 4.dp))
    val current = selected?.takeIf { id -> usable.any { it.id == id } }
        ?: caps.defaultProfile.takeIf { id -> usable.any { it.id == id } }
        ?: usable.first().id
    Segmented(
        options = usable.map { SegmentedOption(value = it.id, label = it.label, description = sustainText(it)) },
        selected = current,
        onSelect = { onSelect(it) },
        small = true,
    )
    usable.firstOrNull { it.id == current }?.let { profile ->
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Pill(sustainText(profile), tone = if (profile.measuredSustain == "true") PillTone.OK else PillTone.NEUTRAL)
            Text(
                "${profile.outputWidth}×${profile.outputHeight}",
                style = MaterialTheme.typography.bodySmall.copy(fontFamily = TatakaiFonts.jetBrainsMono),
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun sustainText(profile: LinkProfile): String = stringResource(
    when (profile.measuredSustain) {
        "true" -> R.string.linked_sustain_ok
        "false" -> R.string.linked_sustain_no
        else -> R.string.linked_sustain_unknown
    },
)

@Composable
private fun RemotePanel(onForget: () -> Unit) {
    Panel(icon = Icons.Outlined.PhoneAndroid, title = stringResource(R.string.linked_remote)) {
        Hint(stringResource(R.string.linked_remote_m1))
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            FilledTonalIconButton(onClick = {}, enabled = false) {
                Icon(Icons.Filled.SkipPrevious, contentDescription = stringResource(R.string.linked_remote_prev))
            }
            FilledIconButton(onClick = {}, enabled = false, modifier = Modifier.size(56.dp)) {
                Icon(Icons.Filled.PlayArrow, contentDescription = stringResource(R.string.linked_remote_play))
            }
            FilledTonalIconButton(onClick = {}, enabled = false) {
                Icon(Icons.Filled.SkipNext, contentDescription = stringResource(R.string.linked_remote_next))
            }
        }
        ForgetButton(onForget)
    }
}

@Composable
private fun ForgetButton(onForget: () -> Unit) {
    var confirming by rememberSaveable { mutableStateOf(false) }
    OutlinedButton(onClick = { confirming = true }) {
        Icon(Icons.Outlined.Delete, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
        Text(stringResource(R.string.linked_forget), modifier = Modifier.padding(start = 8.dp))
    }
    if (confirming) {
        AlertDialog(
            onDismissRequest = { confirming = false },
            icon = { Icon(Icons.Outlined.Shield, contentDescription = null) },
            title = { Text(stringResource(R.string.linked_forget_title)) },
            text = { Text(stringResource(R.string.linked_forget_body)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirming = false
                        onForget()
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) { Text(stringResource(R.string.linked_forget)) }
            },
            dismissButton = { TextButton(onClick = { confirming = false }) { Text(stringResource(UiR.string.ui_cancel)) } },
        )
    }
}

@Composable
private fun EnhancePanel(state: LinkUiState, onStart: (String) -> Unit) {
    var url by rememberSaveable { mutableStateOf("") }
    Panel(icon = Icons.Outlined.Cast, title = stringResource(R.string.linked_enhance_title)) {
        OutlinedTextField(
            value = url,
            onValueChange = { url = it },
            label = { Text(stringResource(R.string.linked_enhance_url)) },
            supportingText = { Text(stringResource(R.string.linked_enhance_hint)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth(),
        )
        state.sessionError?.let {
            Text(it.userMessage(), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
        }
        Button(
            onClick = { onStart(url) },
            enabled = !state.startingSession && url.isNotBlank() && state.capabilities is LoadState.Ok,
        ) {
            if (state.startingSession) LoadingIndicator(Modifier.padding(end = 8.dp).size(20.dp))
            Text(stringResource(if (state.startingSession) R.string.linked_enhance_starting else R.string.linked_enhance_start))
        }
    }
}

// ---------------------------------------------------------------- PC extension server entry (docs/redesign/13 §3.10)

@Composable
private fun ExtensionServerCard(onOpen: () -> Unit) {
    Panel(icon = Icons.Outlined.Extension, title = stringResource(R.string.extserver_entry_title)) {
        Hint(stringResource(R.string.extserver_entry_body))
        FilledTonalButton(onClick = onOpen) {
            Icon(Icons.Outlined.QrCode, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            Text(stringResource(R.string.extserver_entry_open), modifier = Modifier.padding(start = 8.dp))
        }
    }
}

// ---------------------------------------------------------------- building blocks

/** Prototype `.panel` with a titled header. */
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
private fun Cap(key: String, value: String) {
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(
            key,
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.weight(1f),
        )
        Text(
            value,
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1.4f),
        )
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
}
