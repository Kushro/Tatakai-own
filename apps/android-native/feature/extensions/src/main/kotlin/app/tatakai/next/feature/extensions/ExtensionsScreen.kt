package app.tatakai.next.feature.extensions

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.model.ExtensionCompat
import app.tatakai.next.model.InstalledExtension
import app.tatakai.next.ui.R as UiR
import org.koin.compose.viewmodel.koinViewModel

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ExtensionsScreen(
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    viewModel: ExtensionsViewModel = koinViewModel(),
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    // SAF: the user picks the .kai package; no storage permission needed. Android may not know the .kai type, so allow any.
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) viewModel.install(uri.toString())
    }
    var pendingRemove by remember { mutableStateOf<InstalledExtension?>(null) }

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
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { if (!state.installing) picker.launch(arrayOf("*/*")) },
                icon = { Icon(Icons.Filled.Add, contentDescription = null) },
                text = {
                    Text(stringResource(if (state.installing) R.string.extensions_installing else R.string.extensions_install))
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            state.message?.let { MessageBanner(it, viewModel::dismissMessage) }
            if (state.items.isEmpty()) {
                EmptyState(
                    icon = Icons.Outlined.Extension,
                    title = stringResource(R.string.extensions_empty_title),
                    message = stringResource(R.string.extensions_empty_message),
                    modifier = Modifier.weight(1f),
                )
            } else {
                LazyColumn(
                    contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 96.dp),
                    verticalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    items(state.items, key = { it.id }, contentType = { "extension" }) { ext ->
                        ExtensionCard(ext, onToggle = { viewModel.setEnabled(ext.id, it) }, onRemove = { pendingRemove = ext })
                    }
                }
            }
        }
    }

    pendingRemove?.let { ext ->
        AlertDialog(
            onDismissRequest = { pendingRemove = null },
            title = { Text(stringResource(R.string.extensions_remove_title, ext.name)) },
            text = { Text(stringResource(R.string.extensions_remove_message)) },
            confirmButton = {
                TextButton(onClick = {
                    viewModel.remove(ext.id)
                    pendingRemove = null
                }) { Text(stringResource(R.string.extensions_remove)) }
            },
            dismissButton = { TextButton(onClick = { pendingRemove = null }) { Text(stringResource(UiR.string.ui_close)) } },
        )
    }
}

@Composable
private fun MessageBanner(message: InstallMessage, onDismiss: () -> Unit) {
    val failed = message is InstallMessage.Failed
    Surface(
        color = if (failed) MaterialTheme.colorScheme.errorContainer else MaterialTheme.colorScheme.secondaryContainer,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth().padding(16.dp),
    ) {
        Row(Modifier.padding(start = 16.dp, top = 4.dp, bottom = 4.dp, end = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(
                when (message) {
                    is InstallMessage.Installed -> stringResource(R.string.extensions_installed, message.name)
                    is InstallMessage.Failed -> stringResource(R.string.extensions_install_failed, message.reason)
                },
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.bodyMedium,
            )
            TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_close)) }
        }
    }
}

@Composable
private fun ExtensionCard(ext: InstalledExtension, onToggle: (Boolean) -> Unit, onRemove: () -> Unit) {
    Surface(
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surfaceContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(ext.name, style = MaterialTheme.typography.titleMedium)
                    Text(
                        "${ext.type} • v${ext.version}",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Switch(
                    checked = ext.enabled,
                    onCheckedChange = onToggle,
                    enabled = ext.compat != ExtensionCompat.UNSUPPORTED,
                )
                IconButton(onClick = onRemove) {
                    Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.extensions_remove))
                }
            }
            InfoChip(stringResource(ext.compat.labelRes()))
            if (ext.permissions.isNotEmpty()) {
                Text(
                    stringResource(R.string.extensions_permissions, ext.permissions.joinToString()),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

private fun ExtensionCompat.labelRes(): Int = when (this) {
    ExtensionCompat.FULL -> R.string.extensions_compat_full
    ExtensionCompat.PARTIAL -> R.string.extensions_compat_partial
    ExtensionCompat.UNSUPPORTED -> R.string.extensions_compat_unsupported
}
