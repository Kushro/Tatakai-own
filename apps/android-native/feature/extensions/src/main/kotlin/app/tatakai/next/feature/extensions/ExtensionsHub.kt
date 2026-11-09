// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Link
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.PowerSettingsNew
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Storefront
import androidx.compose.material.icons.outlined.SystemUpdate
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.CustomSourceRef
import app.tatakai.next.model.ExtensionPrefType
import app.tatakai.next.model.ExtensionPreference
import app.tatakai.next.model.ExtensionRepo
import app.tatakai.next.model.ExtensionUpdate
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.StremioAddon
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.relativeTime
import app.tatakai.next.ui.state.userMessage

// Parity wave B (16 §3.4, worker t): the composables of the Extensions hub sections (see ExtensionsHubViewModel).

/** Top bar actions: updates badge (ext.updates.sheet) and the overflow with the kill switch (ext.kill-switch). */
@Composable
internal fun HubTopActions(
    updates: Int,
    anyEnabled: Boolean,
    hasExtensions: Boolean,
    onOpenUpdates: () -> Unit,
    onKillSwitch: (enable: Boolean) -> Unit,
) {
    val updatesLabel = pluralStringResource(R.plurals.extensions_updates_badge, updates, updates)
    IconButton(onClick = onOpenUpdates, modifier = Modifier.semantics { contentDescription = updatesLabel }) {
        if (updates > 0) {
            BadgedBox(badge = { Badge { Text(updates.toString()) } }) { Icon(Icons.Outlined.SystemUpdate, contentDescription = null) }
        } else {
            Icon(Icons.Outlined.SystemUpdate, contentDescription = null)
        }
    }
    if (hasExtensions) {
        var open by remember { mutableStateOf(false) }
        IconButton(onClick = { open = true }) { Icon(Icons.Outlined.MoreVert, contentDescription = stringResource(R.string.extensions_more_actions)) }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(if (anyEnabled) R.string.extensions_disable_all else R.string.extensions_enable_all)) },
                leadingIcon = { Icon(Icons.Outlined.PowerSettingsNew, contentDescription = null) },
                onClick = {
                    open = false
                    onKillSwitch(!anyEnabled)
                },
            )
        }
    }
}

/** Confirmation of "Desactivar todas". */
@Composable
internal fun KillSwitchDialog(onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.PowerSettingsNew, contentDescription = null) },
        title = { Text(stringResource(R.string.extensions_disable_all_title)) },
        text = { Text(stringResource(R.string.extensions_disable_all_message)) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.extensions_disable_all)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_close)) } },
    )
}

/** Sheet "Actualizaciones" (classic ExtensionUpdateSheet): one row per update, "Actualizar todas", check now, check at start. */
@Composable
internal fun UpdatesSheet(state: HubUiState, hub: ExtensionsHubViewModel, onDismiss: () -> Unit) {
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(horizontal = 24.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(stringResource(R.string.extensions_updates_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.extensions_updates_on_start), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                Switch(checked = state.updateCheckOnStart, onCheckedChange = hub::setUpdateCheckOnStart)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = hub::checkUpdates, enabled = !state.checkingUpdates) {
                    Icon(Icons.Outlined.Refresh, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(
                        stringResource(if (state.checkingUpdates) R.string.extensions_updates_checking else R.string.extensions_updates_check),
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
                if (state.updates.size > 1) {
                    Button(onClick = hub::updateAll, enabled = state.updating.isEmpty()) { Text(stringResource(R.string.extensions_updates_all)) }
                }
            }
            when {
                state.checkingUpdates && state.updates.isEmpty() -> if (rememberAfterDelay()) LoadingState(Modifier.fillMaxWidth().height(120.dp))
                state.repos.isEmpty() -> Text(
                    stringResource(R.string.extensions_updates_no_repos),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                state.updates.isEmpty() -> Text(
                    stringResource(R.string.extensions_updates_none),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                else -> Column(
                    Modifier.heightIn(max = 420.dp).verticalScroll(rememberScrollState()),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    state.updates.forEach { u -> UpdateRow(u, busy = u.id in state.updating, onUpdate = { hub.update(u.id) }) }
                }
            }
        }
    }
}

@Composable
private fun UpdateRow(u: ExtensionUpdate, busy: Boolean, onUpdate: () -> Unit) {
    Surface(shape = MaterialTheme.shapes.medium, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(u.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    stringResource(R.string.extensions_update_versions, u.installedVersion, u.availableVersion),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
                u.notes?.takeIf { it.isNotBlank() }?.let {
                    Text(it, style = MaterialTheme.typography.bodySmall, maxLines = 4, overflow = TextOverflow.Ellipsis)
                }
            }
            Button(onClick = onUpdate, enabled = !busy) {
                Text(stringResource(if (busy) R.string.extensions_updating else R.string.extensions_update))
            }
        }
    }
}

/** "Instalar desde URL" (ext.install-url): a `.kai` URL or the `manifest.json` next to it. */
@Composable
internal fun InstallUrlDialog(installing: Boolean, onInstall: (String) -> Unit, onDismiss: () -> Unit) {
    var url by rememberSaveable { mutableStateOf("") }
    val valid = isHttpUrl(url)
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.Link, contentDescription = null) },
        title = { Text(stringResource(R.string.extensions_install_url_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.extensions_install_url_hint), style = MaterialTheme.typography.bodyMedium)
                OutlinedTextField(
                    value = url,
                    onValueChange = { url = it },
                    label = { Text(stringResource(R.string.extensions_url_label)) },
                    singleLine = true,
                    isError = url.isNotBlank() && !valid,
                    supportingText = if (url.isNotBlank() && !valid) {
                        { Text(stringResource(R.string.extensions_url_invalid)) }
                    } else {
                        null
                    },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { onInstall(url.trim()) }, enabled = valid && !installing) {
                Text(stringResource(if (installing) R.string.extensions_installing else R.string.extensions_install_url_confirm))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_close)) } },
    )
}

/** Preferences form of one extension (ext.preferences): TEXT / SECRET / TOGGLE / SELECT; SECRET only shows "saved" + "Remove". */
@Composable
internal fun PreferencesDialog(extensionId: String, extensionName: String, hub: ExtensionsHubViewModel, onDismiss: () -> Unit) {
    val prefs by produceState<LoadState<List<ExtensionPreference>>>(LoadState.Loading, extensionId) {
        value = try {
            LoadState.Ok(hub.preferences(extensionId))
        } catch (e: kotlinx.coroutines.CancellationException) {
            throw e
        } catch (e: Exception) {
            LoadState.Err(e.toError())
        }
    }
    val valuesFlow = remember(extensionId) { hub.preferenceValues(extensionId) }
    val values by valuesFlow.collectAsState(initial = emptyMap())
    val drafts = remember(extensionId) { mutableStateMapOf<String, String>() }
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.Tune, contentDescription = null) },
        title = { Text(stringResource(R.string.extensions_prefs_title, extensionName)) },
        text = {
            Column(Modifier.heightIn(max = 480.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                when (val p = prefs) {
                    LoadState.Loading -> if (rememberAfterDelay()) LoadingState(Modifier.fillMaxWidth().height(120.dp))
                    is LoadState.Err -> Text(p.cause.userMessage(), color = MaterialTheme.colorScheme.error)
                    is LoadState.Ok -> if (p.value.isEmpty()) {
                        Text(stringResource(R.string.extensions_prefs_empty), style = MaterialTheme.typography.bodyMedium)
                    } else {
                        p.value.forEach { pref ->
                            PreferenceField(
                                pref = pref,
                                stored = values[pref.key],
                                draft = drafts[pref.key],
                                onDraft = { drafts[pref.key] = it },
                                onSave = { v -> hub.setPreference(extensionId, pref.key, v) },
                            )
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(onClick = {
                // TEXT / SECRET fields are saved on "Done"; toggles and selects are saved on change.
                drafts.forEach { (k, v) -> hub.setPreference(extensionId, k, v.ifEmpty { null }) }
                onDismiss()
            }) { Text(stringResource(R.string.extensions_prefs_done)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_close)) } },
    )
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PreferenceField(pref: ExtensionPreference, stored: String?, draft: String?, onDraft: (String) -> Unit, onSave: (String?) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        when (pref.type) {
            ExtensionPrefType.TOGGLE -> {
                val on = (stored ?: pref.default).toBoolean()
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(pref.title, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                    Switch(checked = on, onCheckedChange = { onSave(it.toString()) })
                }
            }
            ExtensionPrefType.SELECT -> {
                Text(pref.title, style = MaterialTheme.typography.bodyLarge)
                val current = stored ?: pref.default
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    pref.options.forEach { o -> FilterChip(selected = o == current, onClick = { onSave(o) }, label = { Text(o) }) }
                }
            }
            ExtensionPrefType.TEXT -> OutlinedTextField(
                value = draft ?: stored ?: "",
                onValueChange = onDraft,
                label = { Text(pref.title) },
                placeholder = if (pref.default != null) {
                    { Text(pref.default.orEmpty()) }
                } else {
                    null
                },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            ExtensionPrefType.SECRET -> if (stored != null && draft == null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(pref.title, style = MaterialTheme.typography.bodyLarge)
                        Text(stringResource(R.string.extensions_prefs_secret_saved), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.primary)
                    }
                    TextButton(onClick = { onSave(null) }) { Text(stringResource(R.string.extensions_prefs_secret_remove)) }
                }
            } else {
                OutlinedTextField(
                    value = draft ?: "",
                    onValueChange = onDraft,
                    label = { Text(pref.title) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
        pref.description?.takeIf { it.isNotBlank() }?.let {
            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Providers, repositories and Stremio sections, appended after the installed list. */
internal fun LazyListScope.hubSections(
    state: HubUiState,
    hub: ExtensionsHubViewModel,
    onOpenProvider: (extensionId: String, sourceId: String) -> Unit,
    onRemoveRepo: (ExtensionRepo) -> Unit,
    onRemoveAddon: (StremioAddon) -> Unit,
) {
    item(key = "providers-head", contentType = "head") { SectionHead(stringResource(R.string.extensions_providers_title), stringResource(R.string.extensions_providers_hint)) }
    when (val p = state.providers) {
        LoadState.Loading -> item(key = "providers-loading", contentType = "state") {
            if (rememberAfterDelay()) LoadingState(Modifier.fillMaxWidth().height(96.dp))
        }
        is LoadState.Err -> item(key = "providers-error", contentType = "state") {
            ErrorState(message = p.cause.userMessage(), retryLabel = stringResource(UiR.string.ui_retry), onRetry = hub::loadProviders, compact = true)
        }
        is LoadState.Ok -> if (p.value.isEmpty()) {
            item(key = "providers-empty", contentType = "state") {
                Text(
                    stringResource(R.string.extensions_providers_empty),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            items(p.value, key = { "prov:${it.extensionId}/${it.sourceId}" }, contentType = { "provider" }) { src ->
                ProviderRow(src, onClick = { onOpenProvider(src.extensionId, src.sourceId) })
            }
        }
    }

    item(key = "repos-head", contentType = "head") { SectionHead(stringResource(R.string.extensions_repos_title), stringResource(R.string.extensions_repos_hint)) }
    item(key = "repos-add", contentType = "add") {
        UrlAddRow(
            label = stringResource(R.string.extensions_repos_add_label),
            action = stringResource(R.string.extensions_repos_add),
            busy = state.busy,
            error = state.addRepoError,
            onAdd = hub::addRepository,
            onEdit = hub::clearRepoError,
        )
    }
    if (state.repos.isEmpty()) {
        item(key = "repos-empty", contentType = "state") {
            EmptyState(icon = Icons.Outlined.Storefront, title = stringResource(R.string.extensions_repos_empty_title), message = stringResource(R.string.extensions_repos_empty))
        }
    } else {
        items(state.repos, key = { "repo:${it.url}" }, contentType = { "repo" }) { r ->
            RepoCard(r, busy = state.busy, hub = hub, onRemove = { onRemoveRepo(r) })
        }
    }

    if (state.stremioAvailable) {
        item(key = "stremio-head", contentType = "head") { SectionHead(stringResource(R.string.extensions_stremio_title), stringResource(R.string.extensions_stremio_hint)) }
        item(key = "stremio-add", contentType = "add") {
            UrlAddRow(
                label = stringResource(R.string.extensions_stremio_add_label),
                action = stringResource(R.string.extensions_stremio_add),
                busy = state.busy,
                error = state.addonError,
                onAdd = hub::installAddon,
                onEdit = hub::clearAddonError,
                acceptStremioScheme = true,
            )
        }
        if (state.addons.isEmpty()) {
            item(key = "stremio-empty", contentType = "state") {
                Text(
                    stringResource(R.string.extensions_stremio_empty),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            items(state.addons, key = { "addon:${it.id}" }, contentType = { "addon" }) { a ->
                AddonCard(a, busy = ExtensionsHubViewModel.ADDON_PREFIX + a.id in state.busy, hub = hub, onRemove = { onRemoveAddon(a) })
            }
        }
    }
}

@Composable
internal fun SectionHead(title: String, hint: String) {
    Column(Modifier.padding(top = 16.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
        Text(title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
        Text(hint, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

@Composable
private fun ProviderRow(src: CustomSourceRef, onClick: () -> Unit) {
    Surface(
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surfaceContainer,
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick),
    ) {
        Row(Modifier.padding(16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Icon(Icons.Outlined.Hub, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(src.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    src.extensionName + (src.description?.takeIf { it.isNotBlank() }?.let { " · $it" } ?: ""),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Pill(stringResource(providerKindRes(src.kind)), tone = PillTone.NEUTRAL)
        }
    }
}

internal fun providerKindRes(kind: String): Int = when (kind) {
    "read" -> R.string.extensions_provider_kind_read
    "novel" -> R.string.extensions_provider_kind_novel
    else -> R.string.extensions_provider_kind_watch
}

@Composable
private fun UrlAddRow(
    label: String,
    action: String,
    busy: Set<String>,
    error: String?,
    onAdd: (String) -> Unit,
    onEdit: () -> Unit,
    acceptStremioScheme: Boolean = false,
) {
    var url by rememberSaveable { mutableStateOf("") }
    val clean = url.trim()
    val valid = isHttpUrl(clean) || (acceptStremioScheme && clean.startsWith("stremio://", ignoreCase = true) && clean.length > 11)
    val working = clean in busy
    Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = url,
                onValueChange = {
                    url = it
                    if (error != null) onEdit()
                },
                label = { Text(label) },
                singleLine = true,
                isError = error != null || (clean.isNotEmpty() && !valid),
                modifier = Modifier.weight(1f),
            )
            Button(onClick = { onAdd(clean) }, enabled = valid && !working) {
                Text(if (working) stringResource(R.string.extensions_working) else action)
            }
        }
        when {
            error != null -> Text(stringResource(R.string.extensions_add_failed, error), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
            clean.isNotEmpty() && !valid -> Text(stringResource(R.string.extensions_url_invalid), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
        }
    }
}

@Composable
private fun RepoCard(repo: ExtensionRepo, busy: Set<String>, hub: ExtensionsHubViewModel, onRemove: () -> Unit) {
    val refreshing = repo.url in busy
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(repo.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(repo.url, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    val fetched = repo.fetchedAtMs?.let { relativeTime(it) }
                    Text(
                        pluralStringResource(R.plurals.extensions_repo_count, repo.entries.size, repo.entries.size) +
                            (fetched?.let { " · " + stringResource(R.string.extensions_repo_fetched, it) } ?: ""),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                IconButton(onClick = { hub.refreshRepository(repo.url) }, enabled = !refreshing) {
                    Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.extensions_repo_refresh))
                }
                IconButton(onClick = onRemove) { Icon(Icons.Outlined.Delete, contentDescription = stringResource(R.string.extensions_repo_remove)) }
            }
            repo.error?.let { Text(stringResource(R.string.extensions_repo_error, it), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) }
            repo.entries.forEach { e ->
                val working = e.url in busy
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Column(Modifier.weight(1f)) {
                        Text("${e.name} · v${e.version}", style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        if (e.description.isNotBlank()) {
                            Text(e.description, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis)
                        }
                    }
                    when {
                        e.updateAvailable -> Button(onClick = { hub.update(e.id) }, enabled = !working) { Text(stringResource(R.string.extensions_update)) }
                        e.installed -> Pill(stringResource(R.string.extensions_repo_installed), tone = PillTone.OK)
                        else -> OutlinedButton(onClick = { hub.installEntry(e.url) }, enabled = !working) {
                            Text(stringResource(if (working) R.string.extensions_installing else R.string.extensions_repo_install))
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun AddonCard(addon: StremioAddon, busy: Boolean, hub: ExtensionsHubViewModel, onRemove: () -> Unit) {
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    Text(addon.name, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        stringResource(R.string.extensions_version, addon.version) + (addon.types.takeIf { it.isNotEmpty() }?.let { " · " + it.joinToString(", ") } ?: ""),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                val label = stringResource(R.string.extensions_enabled)
                Switch(checked = addon.enabled, onCheckedChange = { hub.setAddonEnabled(addon.id, it) }, modifier = Modifier.semantics { contentDescription = label })
            }
            if (addon.description.isNotBlank()) {
                Text(addon.description, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 3, overflow = TextOverflow.Ellipsis)
            }
            if (!addon.hasStreams) {
                Text(stringResource(R.string.extensions_stremio_no_streams), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                TextButton(onClick = { hub.updateAddon(addon.id) }, enabled = !busy) {
                    Text(stringResource(if (busy) R.string.extensions_updating else R.string.extensions_stremio_update))
                }
                TextButton(onClick = { hub.resetAddonMatches(addon.id) }) { Text(stringResource(R.string.extensions_stremio_reset)) }
                TextButton(onClick = onRemove) { Text(stringResource(R.string.extensions_remove_confirm), color = MaterialTheme.colorScheme.error) }
            }
        }
    }
}

/** Confirmation for removing a repository or an addon. */
@Composable
internal fun ConfirmRemoveDialog(title: String, message: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        icon = { Icon(Icons.Outlined.Delete, contentDescription = null) },
        title = { Text(title) },
        text = { Text(message) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.extensions_remove_confirm), color = MaterialTheme.colorScheme.error) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_close)) } },
    )
}

/** URL syntax check shared by the dialogs (http / https with a host, no spaces). */
internal fun isHttpUrl(raw: String): Boolean {
    val s = raw.trim()
    val rest = when {
        s.startsWith("https://", ignoreCase = true) -> s.substring(8)
        s.startsWith("http://", ignoreCase = true) -> s.substring(7)
        else -> return false
    }
    val host = rest.substringBefore('/').substringBefore('?').substringBefore('#')
    return host.isNotBlank() && !host.contains(' ') && s.none { it.isWhitespace() }
}
