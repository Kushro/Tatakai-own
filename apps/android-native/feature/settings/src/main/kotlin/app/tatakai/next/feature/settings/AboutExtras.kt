// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.settings

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.NewReleases
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material.icons.outlined.SystemUpdate
import androidx.compose.material.icons.outlined.Terminal
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.core.content.FileProvider
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.designsystem.theme.TatakaiFonts
import app.tatakai.next.model.LoadState
import app.tatakai.next.ui.state.userMessage
import app.tatakai.next.ui.R as UiR

// parity wave A (15 §3.4, worker q; checklist system.update-check, system.open-logs): rows of Settings › Acerca de.

/** "Buscar actualizaciones" against GitHub Releases of Kushro/Tatakai-own; "Al día" or the new version with a link to the release. */
@Composable
internal fun UpdateCheckRow(system: SystemViewModel, updateCheckEnabled: Boolean, onToggle: (Boolean) -> Unit) {
    val state by system.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    SwitchRow(
        label = stringResource(R.string.settings_update_check),
        checked = updateCheckEnabled,
        onChange = onToggle,
        hint = stringResource(R.string.settings_update_check_hint),
    )
    Row(
        Modifier.fillMaxWidth().padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        val update = state.update
        FilledTonalButton(onClick = system::checkUpdates, enabled = update != UpdateCheckState.Checking) {
            if (update == UpdateCheckState.Checking) {
                LoadingIndicator(Modifier.size(20.dp))
            } else {
                Icon(Icons.Outlined.SystemUpdate, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
            }
            Text(stringResource(R.string.settings_update_check_now), modifier = Modifier.padding(start = 8.dp))
        }
        when (update) {
            UpdateCheckState.Idle, UpdateCheckState.Checking -> Unit
            UpdateCheckState.UpToDate -> Pill(stringResource(R.string.settings_update_up_to_date), tone = PillTone.OK)
            is UpdateCheckState.Available -> TextButton(onClick = { context.openUrl(update.info.url) }) {
                Icon(Icons.Outlined.NewReleases, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                Text(stringResource(R.string.settings_update_available, update.info.version), modifier = Modifier.padding(start = 8.dp))
            }
            is UpdateCheckState.Failed -> Text(
                stringResource(R.string.settings_update_failed),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.error,
            )
        }
    }
}

/** "Abrir registros": last 500 lines of the app log (newest at the bottom) + Compartir (FileProvider) + Vaciar. */
@Composable
internal fun LogsSheet(system: SystemViewModel, onDismiss: () -> Unit) {
    val state by system.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val snackbar = LocalAppSnackbar.current
    LaunchedEffect(system) { system.loadLogs() }
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Icon(Icons.Outlined.Terminal, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
            Text(
                stringResource(R.string.settings_logs),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.weight(1f).padding(start = 8.dp),
            )
            val lines = (state.logs as? LoadState.Ok)?.value.orEmpty()
            TextButton(onClick = system::clearLogs, enabled = lines.isNotEmpty()) {
                Icon(Icons.Outlined.DeleteSweep, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                Text(stringResource(R.string.settings_logs_clear), modifier = Modifier.padding(start = 4.dp))
            }
            FilledTonalButton(
                onClick = {
                    if (!context.shareLog(system)) snackbar.show(context.getString(R.string.settings_logs_share_failed))
                },
                enabled = lines.isNotEmpty(),
            ) {
                Icon(Icons.Outlined.Share, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                Text(stringResource(R.string.settings_logs_share), modifier = Modifier.padding(start = 4.dp))
            }
        }
        when (val logs = state.logs) {
            LoadState.Loading -> if (rememberAfterDelay(150)) {
                Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { LoadingIndicator() }
            }
            is LoadState.Err -> ErrorState(
                message = logs.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = system::loadLogs,
            )
            is LoadState.Ok -> if (logs.value.isEmpty()) {
                EmptyState(icon = Icons.Outlined.Terminal, title = stringResource(R.string.settings_logs_empty))
            } else {
                val listState = rememberLazyListState()
                LaunchedEffect(logs.value.size) { listState.scrollToItem(logs.value.lastIndex) }
                LazyColumn(
                    state = listState,
                    modifier = Modifier.fillMaxWidth().heightIn(max = 560.dp),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(2.dp),
                ) {
                    items(logs.value) { line ->
                        Text(
                            line,
                            style = MaterialTheme.typography.bodySmall.copy(fontFamily = TatakaiFonts.jetBrainsMono),
                            color = when {
                                " E/" in line -> MaterialTheme.colorScheme.error
                                " W/" in line -> MaterialTheme.colorScheme.tertiary
                                else -> MaterialTheme.colorScheme.onSurface
                            },
                        )
                    }
                }
            }
        }
    }
}

/** Shares `filesDir/logs/app.log` through the `${applicationId}.logs` FileProvider declared by :core:data. */
private fun Context.shareLog(system: SystemViewModel): Boolean {
    val file = system.logFile() ?: return false
    return try {
        val uri = FileProvider.getUriForFile(this, "$packageName.logs", file)
        val send = Intent(Intent.ACTION_SEND)
            .setType("text/plain")
            .putExtra(Intent.EXTRA_STREAM, uri)
            .putExtra(Intent.EXTRA_SUBJECT, "Tatakai Next log")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        startActivity(Intent.createChooser(send, getString(R.string.settings_logs_share)))
        true
    } catch (e: IllegalArgumentException) {
        false
    } catch (e: ActivityNotFoundException) {
        false
    }
}

internal fun Context.openUrl(url: String) {
    try {
        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
    } catch (_: ActivityNotFoundException) {
        // no browser: nothing to do, the version is already shown
    }
}
