// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.extensionserver.ServerExtension
import app.tatakai.next.ui.R as UiR

// Parity wave B closure (16 §3.4, worker t; checklist server.api.extensions): "Extensions on your PC" section of Extensions, one block per
// linked extension server with the list its `GET /extensions` returns. Pairing and removing servers stay in Linked devices.

internal fun LazyListScope.serverExtensionSections(state: ServerExtensionsUiState, vm: ServerExtensionsViewModel) {
    if (!state.available) return
    item(key = "srvext-head", contentType = "head") {
        SectionHead(stringResource(R.string.extensions_server_title), stringResource(R.string.extensions_server_hint))
    }
    if (!state.loaded) {
        item(key = "srvext-loading", contentType = "state") {
            if (rememberAfterDelay()) LoadingState(Modifier.fillMaxWidth().height(96.dp))
        }
        return
    }
    if (state.servers.isEmpty()) {
        item(key = "srvext-empty", contentType = "state") {
            Text(
                stringResource(R.string.extensions_server_none),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        return
    }
    state.servers.forEach { block ->
        item(key = "srvext-server:${block.id}", contentType = "server-head") {
            ServerHead(block, onRefresh = { vm.refresh(block.id) })
        }
        when (val load = block.load) {
            ServerExtensionsLoad.Loading -> item(key = "srvext-loading:${block.id}", contentType = "state") {
                if (rememberAfterDelay()) LoadingState(Modifier.fillMaxWidth().height(72.dp))
            }
            ServerExtensionsLoad.Disabled -> item(key = "srvext-disabled:${block.id}", contentType = "state") {
                Text(
                    stringResource(R.string.extensions_server_disabled),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            is ServerExtensionsLoad.Err -> item(key = "srvext-error:${block.id}", contentType = "state") {
                ErrorState(
                    message = stringResource(serverExtErrorRes(load.error)),
                    retryLabel = stringResource(UiR.string.ui_retry),
                    onRetry = { vm.refresh(block.id) },
                    compact = true,
                )
            }
            is ServerExtensionsLoad.Ok -> if (load.extensions.isEmpty()) {
                item(key = "srvext-none:${block.id}", contentType = "state") {
                    Text(
                        stringResource(R.string.extensions_server_empty),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            } else {
                items(load.extensions, key = { "srvext:${block.id}/${it.id}" }, contentType = { "server-ext" }) { ext ->
                    ServerExtensionRow(ext)
                }
            }
        }
    }
}

@Composable
private fun ServerHead(block: ServerExtensionsBlock, onRefresh: () -> Unit) {
    Row(Modifier.fillMaxWidth().padding(top = 4.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Icon(Icons.Outlined.Computer, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(block.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            val count = (block.load as? ServerExtensionsLoad.Ok)?.extensions?.size
            Text(
                if (count != null) pluralStringResource(R.plurals.extensions_server_count, count, count) else block.url,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (block.load !is ServerExtensionsLoad.Disabled) {
            IconButton(onClick = onRefresh, enabled = block.load !is ServerExtensionsLoad.Loading) {
                Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.extensions_server_refresh, block.name))
            }
        }
    }
}

@Composable
private fun ServerExtensionRow(ext: ServerExtension) {
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(Icons.Outlined.Extension, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(ext.name.ifBlank { ext.id }, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                val meta = listOfNotNull(
                    ext.version.takeIf { it.isNotBlank() }?.let { stringResource(R.string.extensions_server_version, it) },
                    ext.ecosystem.takeIf { it.isNotBlank() },
                    serverExtensionKinds(ext.kind).takeIf { it.isNotEmpty() }?.map { stringResource(providerKindRes(it)) }?.joinToString(", "),
                ).joinToString(" · ")
                if (meta.isNotEmpty()) {
                    Text(meta, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
            Pill(
                stringResource(if (ext.enabled) R.string.extensions_server_on else R.string.extensions_server_off),
                tone = if (ext.enabled) PillTone.OK else PillTone.NEUTRAL,
            )
        }
    }
}

internal fun serverExtErrorRes(error: ServerExtError): Int = when (error) {
    ServerExtError.UNREACHABLE -> R.string.extensions_server_err_unreachable
    ServerExtError.UNAUTHORIZED -> R.string.extensions_server_err_unauthorized
    ServerExtError.INCOMPATIBLE -> R.string.extensions_server_err_incompatible
    ServerExtError.OTHER -> R.string.extensions_server_err_other
}
