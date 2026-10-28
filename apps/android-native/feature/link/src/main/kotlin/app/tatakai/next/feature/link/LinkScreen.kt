package app.tatakai.next.feature.link

import android.Manifest
import android.content.pm.PackageManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.QrCodeScanner
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.designsystem.components.LinkStatusAvatar
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.model.LinkCapabilities
import app.tatakai.next.model.LinkState
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.state.userMessage
import org.koin.compose.viewmodel.koinViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun LinkScreen(
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
    viewModel: LinkViewModel = koinViewModel(),
) {
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
                title = { Text(stringResource(R.string.link_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
                    }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.fillMaxSize().padding(padding).verticalScroll(rememberScrollState()).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            StatusCard(state.link, onForget = viewModel::forget)
            val link = state.link
            if (link is LinkState.Paired) {
                SessionCard(
                    state = state,
                    onSelectProfile = viewModel::selectProfile,
                    onRetryCapabilities = viewModel::retryCapabilities,
                    onStart = viewModel::startSession,
                )
            } else {
                PairCard(state.pairing, state.pairError?.userMessageOrNull(), viewModel::pair)
            }
        }
    }
}

@Composable
private fun app.tatakai.next.model.AppError.userMessageOrNull(): String = userMessage()

@Composable
private fun StatusCard(link: LinkState, onForget: () -> Unit) {
    SectionCard {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            val connected = link is LinkState.Paired && link.reachable
            LinkStatusAvatar(connected = connected, icon = Icons.Outlined.Computer)
            Column(Modifier.weight(1f)) {
                when (link) {
                    LinkState.Unpaired -> {
                        Text(stringResource(R.string.link_status_unpaired), style = MaterialTheme.typography.titleMedium)
                        Text(stringResource(R.string.link_unpaired_hint), style = MaterialTheme.typography.bodyMedium)
                    }
                    LinkState.Pairing -> Text(stringResource(R.string.link_status_pairing), style = MaterialTheme.typography.titleMedium)
                    is LinkState.Paired -> {
                        Text(link.pc.serverName, style = MaterialTheme.typography.titleMedium)
                        Text(
                            "${link.pc.host}:${link.pc.port}",
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                        InfoChip(stringResource(if (link.reachable) R.string.link_reachable else R.string.link_unreachable))
                    }
                    is LinkState.Failed -> {
                        Text(stringResource(R.string.link_status_failed), style = MaterialTheme.typography.titleMedium)
                        Text(link.message, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                    }
                }
            }
        }
        if (link is LinkState.Paired) {
            TextButton(onClick = onForget) { Text(stringResource(R.string.link_forget)) }
        }
    }
}

@Composable
private fun PairCard(pairing: Boolean, error: String?, onPair: (String) -> Unit) {
    val context = LocalContext.current
    var scanning by rememberSaveable { mutableStateOf(false) }
    var denied by rememberSaveable { mutableStateOf(false) }
    var code by rememberSaveable { mutableStateOf("") }

    val permission = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        denied = !granted
        scanning = granted
    }

    SectionCard(title = stringResource(R.string.link_pair_title)) {
        Text(stringResource(R.string.link_pair_hint), style = MaterialTheme.typography.bodyMedium)
        if (scanning) {
            QrScanner(
                onResult = {
                    scanning = false
                    onPair(it)
                },
                modifier = Modifier.fillMaxWidth().padding(top = 12.dp).aspectRatio(1f).clip(MaterialTheme.shapes.large),
            )
            TextButton(onClick = { scanning = false }) { Text(stringResource(R.string.link_scan_cancel)) }
        } else {
            Button(
                onClick = {
                    val granted = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) ==
                        PackageManager.PERMISSION_GRANTED
                    if (granted) scanning = true else permission.launch(Manifest.permission.CAMERA)
                },
                enabled = !pairing,
                modifier = Modifier.padding(top = 12.dp),
            ) {
                Icon(Icons.Outlined.QrCodeScanner, contentDescription = null)
                Text(stringResource(R.string.link_scan), modifier = Modifier.padding(start = 8.dp))
            }
        }
        if (denied) {
            Text(
                stringResource(R.string.link_camera_denied),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
                modifier = Modifier.padding(top = 8.dp),
            )
        }
        OutlinedTextField(
            value = code,
            onValueChange = { code = it },
            label = { Text(stringResource(R.string.link_paste_label)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
        )
        OutlinedButton(
            onClick = { onPair(code) },
            enabled = !pairing && code.isNotBlank(),
            modifier = Modifier.padding(top = 8.dp),
        ) {
            Text(stringResource(if (pairing) R.string.link_pairing else R.string.link_pair))
        }
        if (error != null) {
            Text(error, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(top = 8.dp))
        }
    }
}

@Composable
private fun SessionCard(
    state: LinkUiState,
    onSelectProfile: (String?) -> Unit,
    onRetryCapabilities: () -> Unit,
    onStart: (String) -> Unit,
) {
    var url by rememberSaveable { mutableStateOf("") }
    SectionCard(title = stringResource(R.string.link_profiles_title)) {
        when (val caps = state.capabilities) {
            null, LoadState.Loading -> Row(Modifier.fillMaxWidth().padding(16.dp), horizontalArrangement = Arrangement.Center) {
                LoadingState(Modifier.aspectRatio(3f))
            }
            is LoadState.Err -> {
                Text(caps.cause.userMessage(), color = MaterialTheme.colorScheme.error)
                TextButton(onClick = onRetryCapabilities) { Text(stringResource(UiR.string.ui_retry)) }
            }
            is LoadState.Ok -> ProfilePicker(caps.value, state.selectedProfile, onSelectProfile)
        }
        OutlinedTextField(
            value = url,
            onValueChange = { url = it },
            label = { Text(stringResource(R.string.link_session_url)) },
            supportingText = { Text(stringResource(R.string.link_session_hint)) },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(top = 12.dp),
        )
        state.sessionError?.let {
            Text(it.userMessage(), color = MaterialTheme.colorScheme.error, modifier = Modifier.padding(top = 8.dp))
        }
        Button(
            onClick = { onStart(url) },
            enabled = !state.startingSession && url.isNotBlank() && state.capabilities is LoadState.Ok,
            modifier = Modifier.padding(top = 8.dp),
        ) {
            Text(stringResource(if (state.startingSession) R.string.link_starting else R.string.link_start))
        }
    }
}

@Composable
private fun ProfilePicker(caps: LinkCapabilities, selected: String?, onSelect: (String?) -> Unit) {
    val current = selected ?: caps.defaultProfile
    Column(Modifier.fillMaxWidth()) {
        caps.profiles.forEach { profile ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .selectable(selected = profile.id == current, role = Role.RadioButton, onClick = { onSelect(profile.id) })
                    .padding(vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                RadioButton(selected = profile.id == current, onClick = null)
                Column(Modifier.padding(start = 12.dp)) {
                    Text(profile.label, style = MaterialTheme.typography.bodyLarge)
                    Text(
                        stringResource(
                            when (profile.measuredSustain) {
                                "true" -> R.string.link_sustain_ok
                                "false" -> R.string.link_sustain_no
                                else -> R.string.link_sustain_unknown
                            },
                        ),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}
