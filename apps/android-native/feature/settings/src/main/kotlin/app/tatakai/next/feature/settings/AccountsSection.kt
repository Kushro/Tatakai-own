// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.settings

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.CloudSync
import androidx.compose.material.icons.outlined.FileDownload
import androidx.compose.material.icons.outlined.FileUpload
import androidx.compose.material.icons.outlined.LinkOff
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.SyncProblem
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.data.InitialSyncMode
import app.tatakai.next.model.ConflictChoice
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListConflict
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import app.tatakai.next.ui.format.relativeTime
import app.tatakai.next.ui.labels.labelRes
import app.tatakai.next.ui.media.MediaCover
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.koin.compose.viewmodel.koinViewModel
import java.time.LocalDate
import java.time.format.DateTimeFormatter

/**
 * Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.3). Owner: worker q.
 *
 * Settings › Listas y cuentas (`SettingsSection.ACCOUNTS`, classic Integrations panel): the app's only identity (2026-11-09: no own
 * login). One card per provider (AniList, MAL) with "Vincular" (Custom Tab on `ListsRepository.beginLink`; disabled with the reason when
 * the build has no client id), the linked username / avatar and "Desvincular"; pending-changes counter and "Sincronizar listas ahora";
 * conflicts with Keep local / Keep remote; auto-progress / auto-add switches; Import / Export (SAF, the desktop `ListEntry[]` JSON);
 * the note that the history is not part of the account. State lives in [AccountsViewModel] (the redirect from
 * `tatakai://oauth/<provider>` arrives through `PendingOAuthRedirect`).
 */
@Composable
internal fun AccountsSection(state: SettingsUiState, vm: SettingsViewModel) {
    val accountsVm: AccountsViewModel = koinViewModel()
    val ui by accountsVm.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val snackbar = LocalAppSnackbar.current
    val scope = rememberCoroutineScope()
    var pendingExport by remember { mutableStateOf<String?>(null) }
    var confirmUnlink by rememberSaveable { mutableStateOf<ListProvider?>(null) }

    val exportLauncher = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        val json = pendingExport
        pendingExport = null
        if (uri == null || json == null) return@rememberLauncherForActivityResult
        scope.launch {
            val ok = withContext(Dispatchers.IO) {
                runCatching { context.contentResolver.openOutputStream(uri, "wt")?.use { it.write(json.toByteArray()) } != null }.getOrDefault(false)
            }
            snackbar.show(context.getString(if (ok) R.string.settings_accounts_exported else R.string.settings_accounts_export_failed))
        }
    }
    val importLauncher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val json = withContext(Dispatchers.IO) {
                runCatching {
                    context.contentResolver.openInputStream(uri)?.use { input ->
                        // a list export is small; refuse anything absurd instead of reading it into memory
                        val bytes = input.readNBytesCompat(MAX_IMPORT_BYTES + 1)
                        if (bytes.size > MAX_IMPORT_BYTES) null else String(bytes)
                    }
                }.getOrNull()
            }
            accountsVm.import(json)
        }
    }

    val currentSnackbar by rememberUpdatedState(snackbar)
    LaunchedEffect(accountsVm) {
        accountsVm.events.collect { event ->
            when (event) {
                is AccountsEvent.OpenUrl -> if (!context.openAuthUrl(event.url)) {
                    accountsVm.linkTabClosed()
                    currentSnackbar.show(context.getString(R.string.settings_accounts_no_browser))
                }
                is AccountsEvent.Linked -> currentSnackbar.show(
                    context.getString(R.string.settings_accounts_linked, event.account.username, context.getString(event.account.provider.labelRes())),
                )
                is AccountsEvent.Unlinked -> currentSnackbar.show(
                    context.getString(R.string.settings_accounts_unlinked, context.getString(event.provider.labelRes())),
                )
                is AccountsEvent.Synced -> currentSnackbar.show(
                    context.getString(R.string.settings_accounts_synced, event.report.pushed, event.report.pulled, event.report.conflicts),
                )
                is AccountsEvent.Exported -> {
                    pendingExport = event.json
                    exportLauncher.launch("tatakai-lists-${LocalDate.now().format(DateTimeFormatter.ISO_LOCAL_DATE)}.json")
                }
                is AccountsEvent.Imported -> currentSnackbar.show(
                    context.resources.getQuantityString(R.plurals.settings_accounts_imported, event.count, event.count),
                )
                is AccountsEvent.Failed -> currentSnackbar.show(context.getString(event.error.messageRes()))
            }
        }
    }
    // Back from the Custom Tab without a redirect: stop the spinner after a moment (a late redirect still completes).
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        if (ui.linking != null) {
            scope.launch {
                delay(TAB_CLOSED_GRACE_MS)
                accountsVm.linkTabClosed()
            }
        }
    }

    SectionHint(stringResource(R.string.settings_accounts_hint))
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        ListProvider.entries.forEach { provider ->
            ProviderCard(
                provider = provider,
                account = ui.accounts.firstOrNull { it.provider == provider },
                configured = provider in ui.configured,
                linking = ui.linking == provider,
                error = ui.linkError?.takeIf { it.first == provider || it.first == null }?.second,
                onLink = { accountsVm.link(provider) },
                onUnlink = { confirmUnlink = provider },
                firstSyncPending = provider in ui.firstSyncPending,
                onSyncMode = { accountsVm.askSyncMode(provider) }.takeIf { !ui.syncing },
            )
        }
    }
    SyncModeSheet(ui, accountsVm)

    // queue + sync
    SettingRow(
        label = stringResource(R.string.settings_accounts_sync),
        hint = when {
            ui.accounts.isEmpty() -> stringResource(R.string.settings_accounts_local_only)
            ui.lastSyncedAt != null -> stringResource(R.string.settings_accounts_last_sync, relativeTime(ui.lastSyncedAt!!))
            else -> stringResource(R.string.settings_accounts_never_synced)
        },
    ) {
        if (ui.pendingChanges > 0) {
            Pill(
                context.resources.getQuantityString(R.plurals.settings_accounts_pending, ui.pendingChanges, ui.pendingChanges),
                tone = PillTone.WARN,
            )
        } else if (ui.accounts.isNotEmpty()) {
            Pill(stringResource(R.string.settings_accounts_up_to_date), tone = PillTone.OK)
        }
    }
    FilledTonalButton(onClick = accountsVm::syncNow, enabled = ui.accounts.isNotEmpty() && !ui.syncing) {
        if (ui.syncing) {
            LoadingIndicator(Modifier.size(20.dp))
        } else {
            Icon(Icons.Outlined.CloudSync, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
        }
        Text(
            stringResource(if (ui.syncing) R.string.settings_syncing else R.string.settings_accounts_sync_now),
            modifier = Modifier.padding(start = 8.dp),
        )
    }

    if (ui.conflicts.isNotEmpty()) ConflictsBlock(ui.conflicts, accountsVm)

    SwitchRow(
        label = stringResource(R.string.settings_accounts_auto_progress),
        checked = state.settings.autoListProgress,
        onChange = { on -> vm.updateSettings { it.copy(autoListProgress = on) } },
        hint = stringResource(R.string.settings_accounts_auto_progress_hint),
    )
    SwitchRow(
        label = stringResource(R.string.settings_accounts_auto_add),
        checked = state.settings.autoListAdd,
        onChange = { on -> vm.updateSettings { it.copy(autoListAdd = on) } },
        hint = stringResource(R.string.settings_accounts_auto_add_hint),
    )
    SwitchRow(
        label = stringResource(R.string.settings_accounts_write_back),
        checked = state.settings.listsWriteBack,
        onChange = { on -> vm.updateSettings { it.copy(listsWriteBack = on) } },
        hint = stringResource(R.string.settings_accounts_write_back_hint),
    )

    SettingRow(stringResource(R.string.settings_accounts_transfer), stringResource(R.string.settings_accounts_transfer_hint), stack = true) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = accountsVm::export, enabled = !ui.transferring) {
                Icon(Icons.Outlined.FileUpload, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                Text(stringResource(R.string.settings_accounts_export), modifier = Modifier.padding(start = 8.dp))
            }
            OutlinedButton(onClick = { importLauncher.launch(arrayOf("application/json", "text/plain", "application/octet-stream")) }, enabled = !ui.transferring) {
                if (ui.transferring) {
                    LoadingIndicator(Modifier.size(20.dp))
                } else {
                    Icon(Icons.Outlined.FileDownload, contentDescription = null, modifier = Modifier.size(ButtonDefaults.IconSize))
                }
                Text(stringResource(R.string.settings_accounts_import), modifier = Modifier.padding(start = 8.dp))
            }
        }
    }
    Text(
        stringResource(R.string.settings_accounts_history_note),
        style = MaterialTheme.typography.bodySmall,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(top = 8.dp),
    )

    confirmUnlink?.let { provider ->
        val name = stringResource(provider.labelRes())
        AlertDialog(
            onDismissRequest = { confirmUnlink = null },
            icon = { Icon(Icons.Outlined.LinkOff, contentDescription = null) },
            title = { Text(stringResource(R.string.settings_accounts_unlink_title, name)) },
            text = { Text(stringResource(R.string.settings_accounts_unlink_body, name)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        confirmUnlink = null
                        accountsVm.unlink(provider)
                    },
                    colors = ButtonDefaults.textButtonColors(contentColor = MaterialTheme.colorScheme.error),
                ) { Text(stringResource(R.string.settings_accounts_unlink)) }
            },
            dismissButton = {
                TextButton(onClick = { confirmUnlink = null }) { Text(stringResource(app.tatakai.next.ui.R.string.ui_cancel)) }
            },
        )
    }
}

/** First-run wizard variant: the provider cards and the OAuth round trip only (unlink without the confirmation dialog). */
@Composable
internal fun OnboardingProviderCards(accountsVm: AccountsViewModel) {
    val ui by accountsVm.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val snackbar = LocalAppSnackbar.current
    val scope = rememberCoroutineScope()
    val currentSnackbar by rememberUpdatedState(snackbar)
    LaunchedEffect(accountsVm) {
        accountsVm.events.collect { event ->
            when (event) {
                is AccountsEvent.OpenUrl -> if (!context.openAuthUrl(event.url)) {
                    accountsVm.linkTabClosed()
                    currentSnackbar.show(context.getString(R.string.settings_accounts_no_browser))
                }
                is AccountsEvent.Linked -> currentSnackbar.show(
                    context.getString(R.string.settings_accounts_linked, event.account.username, context.getString(event.account.provider.labelRes())),
                )
                is AccountsEvent.Failed -> currentSnackbar.show(context.getString(event.error.messageRes()))
                else -> Unit
            }
        }
    }
    LifecycleEventEffect(Lifecycle.Event.ON_RESUME) {
        if (ui.linking != null) {
            scope.launch {
                delay(TAB_CLOSED_GRACE_MS)
                accountsVm.linkTabClosed()
            }
        }
    }
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        ListProvider.entries.forEach { provider ->
            ProviderCard(
                provider = provider,
                account = ui.accounts.firstOrNull { it.provider == provider },
                configured = provider in ui.configured,
                linking = ui.linking == provider,
                error = ui.linkError?.takeIf { it.first == provider || it.first == null }?.second,
                onLink = { accountsVm.link(provider) },
                onUnlink = { accountsVm.unlink(provider) },
                firstSyncPending = provider in ui.firstSyncPending,
                onSyncMode = { accountsVm.askSyncMode(provider) }.takeIf { !ui.syncing },
            )
        }
    }
    SyncModeSheet(ui, accountsVm)
}

/**
 * `lists.import-export`: how to sync a linked provider — merge (default), import the remote list over the local one, or export the
 * local list over the remote one. Opens by itself after linking while the local list has titles (the first sync waits for it), and
 * from the card's "Importar / exportar" at any time. "Más tarde" keeps a pending first sync pending.
 */
@Composable
private fun SyncModeSheet(ui: AccountsUiState, accountsVm: AccountsViewModel) {
    val provider = ui.syncModeSheet ?: return
    val name = stringResource(provider.labelRes())
    val first = provider in ui.firstSyncPending
    AlertDialog(
        onDismissRequest = accountsVm::dismissSyncMode,
        icon = { Icon(Icons.Outlined.CloudSync, contentDescription = null) },
        title = { Text(stringResource(R.string.settings_accounts_sync_mode_title, name)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    stringResource(if (first) R.string.settings_accounts_sync_mode_body_first else R.string.settings_accounts_sync_mode_body, name),
                    style = MaterialTheme.typography.bodyMedium,
                )
                SyncModeOption(
                    title = stringResource(R.string.settings_accounts_mode_merge),
                    hint = stringResource(R.string.settings_accounts_mode_merge_hint),
                    primary = true,
                    onClick = { accountsVm.syncWithMode(provider, InitialSyncMode.MERGE) },
                )
                SyncModeOption(
                    title = stringResource(R.string.settings_accounts_mode_import, name),
                    hint = stringResource(R.string.settings_accounts_mode_import_hint, name),
                    onClick = { accountsVm.syncWithMode(provider, InitialSyncMode.IMPORT) },
                )
                SyncModeOption(
                    title = stringResource(R.string.settings_accounts_mode_export, name),
                    hint = stringResource(R.string.settings_accounts_mode_export_hint, name),
                    onClick = { accountsVm.syncWithMode(provider, InitialSyncMode.EXPORT) },
                )
            }
        },
        confirmButton = {
            TextButton(onClick = accountsVm::dismissSyncMode) {
                Text(stringResource(if (first) R.string.settings_accounts_mode_later else app.tatakai.next.ui.R.string.ui_cancel))
            }
        },
    )
}

@Composable
private fun SyncModeOption(title: String, hint: String, onClick: () -> Unit, primary: Boolean = false) {
    Surface(
        onClick = onClick,
        color = if (primary) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(title, style = MaterialTheme.typography.titleSmall)
            Text(hint, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun ProviderCard(
    provider: ListProvider,
    account: LinkedAccount?,
    configured: Boolean,
    linking: Boolean,
    error: AccountsError?,
    onLink: () -> Unit,
    onUnlink: () -> Unit,
    firstSyncPending: Boolean = false,
    /** Opens the import / export / merge sheet (lists.import-export); null hides the action (no account, or a sync running). */
    onSyncMode: (() -> Unit)? = null,
) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                if (account?.avatar != null) {
                    MediaCover(url = account.avatar, modifier = Modifier.width(40.dp), shape = CircleShape, aspectRatio = 1f)
                } else {
                    Box(
                        Modifier.size(40.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
                        contentAlignment = Alignment.Center,
                    ) { Icon(Icons.Outlined.Person, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimaryContainer) }
                }
                Column(Modifier.weight(1f)) {
                    Text(stringResource(provider.labelRes()), style = MaterialTheme.typography.titleSmall)
                    Text(
                        when {
                            account != null -> stringResource(R.string.settings_accounts_signed_as, account.username)
                            !configured -> stringResource(R.string.settings_accounts_no_client_id)
                            else -> stringResource(R.string.settings_accounts_not_linked)
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                when {
                    account != null -> TextButton(onClick = onUnlink) { Text(stringResource(R.string.settings_accounts_unlink)) }
                    linking -> LoadingIndicator(Modifier.size(32.dp))
                    // Without a client id the button stays visible but disabled; the line above says why (account.oauth.errors).
                    else -> FilledTonalButton(onClick = onLink, enabled = configured) { Text(stringResource(R.string.settings_accounts_link)) }
                }
            }
            if (account != null && (firstSyncPending || onSyncMode != null)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (firstSyncPending) Pill(stringResource(R.string.settings_accounts_first_sync_pending), tone = PillTone.WARN)
                    Box(Modifier.weight(1f))
                    if (onSyncMode != null) {
                        TextButton(onClick = onSyncMode) {
                            Text(stringResource(if (firstSyncPending) R.string.settings_accounts_choose else R.string.settings_accounts_sync_mode))
                        }
                    }
                }
            }
            if (account?.expiresAtMs != null && account.expiresAtMs!! < System.currentTimeMillis()) {
                Pill(stringResource(R.string.settings_accounts_expired), tone = PillTone.WARN)
            }
            if (error != null && account == null) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Icon(Icons.Outlined.SyncProblem, contentDescription = null, tint = MaterialTheme.colorScheme.error, modifier = Modifier.size(18.dp))
                    Text(stringResource(error.messageRes()), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                }
            }
        }
    }
}

@Composable
private fun ConflictsBlock(conflicts: List<ListConflict>, vm: AccountsViewModel) {
    Column(Modifier.padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                LocalContext.current.resources.getQuantityString(R.plurals.settings_accounts_conflicts, conflicts.size, conflicts.size),
                style = MaterialTheme.typography.titleSmall,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = { vm.resolveAll(ConflictChoice.KEEP_LOCAL) }) { Text(stringResource(R.string.settings_accounts_keep_all_local)) }
        }
        conflicts.take(MAX_CONFLICTS_SHOWN).forEach { c ->
            Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(c.local.title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        stringResource(R.string.settings_accounts_conflict_local, entrySummary(c.local)),
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Text(
                        stringResource(R.string.settings_accounts_conflict_remote, stringResource(c.provider.labelRes()), entrySummary(c.remote)),
                        style = MaterialTheme.typography.bodySmall,
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        FilledTonalButton(onClick = { vm.resolve(c, ConflictChoice.KEEP_LOCAL) }) {
                            Text(stringResource(R.string.settings_accounts_keep_local))
                        }
                        OutlinedButton(onClick = { vm.resolve(c, ConflictChoice.KEEP_REMOTE) }) {
                            Text(stringResource(R.string.settings_accounts_keep_remote))
                        }
                    }
                }
            }
        }
        if (conflicts.size > MAX_CONFLICTS_SHOWN) {
            Text(
                stringResource(R.string.settings_accounts_conflicts_more, conflicts.size - MAX_CONFLICTS_SHOWN),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun entrySummary(e: ListEntry): String {
    val status = stringResource(e.status.labelRes())
    val progress = e.totalEpisodes?.let { stringResource(R.string.settings_accounts_progress_of, e.progress, it) }
        ?: stringResource(R.string.settings_accounts_progress, e.progress)
    val score = e.score?.takeIf { it > 0 }?.let { stringResource(R.string.settings_accounts_score, it) }
    return listOfNotNull(status, progress, score).joinToString(" · ")
}

internal fun AccountsError.messageRes(): Int = when (this) {
    AccountsError.NO_CLIENT_ID -> R.string.settings_accounts_no_client_id
    AccountsError.CANCELLED -> R.string.settings_accounts_error_cancelled
    AccountsError.INVALID_STATE -> R.string.settings_accounts_error_state
    AccountsError.EXPIRED -> R.string.settings_accounts_error_expired
    AccountsError.EXCHANGE -> R.string.settings_accounts_error_exchange
    AccountsError.DENIED -> R.string.settings_accounts_error_denied
    AccountsError.OFFLINE -> R.string.settings_accounts_error_offline
    AccountsError.RATE_LIMITED -> R.string.settings_accounts_error_rate
    AccountsError.HTTP -> R.string.settings_accounts_error_http
    AccountsError.BAD_FILE -> R.string.settings_accounts_error_file
    AccountsError.NOT_LINKED -> R.string.settings_accounts_error_not_linked
    AccountsError.UNKNOWN -> R.string.settings_action_failed
}

/** Custom Tab first (OAuth must not run in a WebView), any browser second. False when nothing can open it. */
private fun Context.openAuthUrl(url: String): Boolean {
    val uri = Uri.parse(url)
    return try {
        CustomTabsIntent.Builder().setShowTitle(true).build().launchUrl(this, uri)
        true
    } catch (e: ActivityNotFoundException) {
        try {
            startActivity(Intent(Intent.ACTION_VIEW, uri).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            true
        } catch (e2: ActivityNotFoundException) {
            false
        }
    }
}

/** `InputStream.readNBytes` is API 33+; this reads at most [max] bytes on every API level. */
private fun java.io.InputStream.readNBytesCompat(max: Int): ByteArray {
    val out = java.io.ByteArrayOutputStream()
    val buf = ByteArray(8192)
    var total = 0
    while (total < max) {
        val n = read(buf, 0, minOf(buf.size, max - total))
        if (n < 0) break
        out.write(buf, 0, n)
        total += n
    }
    return out.toByteArray()
}

private const val MAX_IMPORT_BYTES = 8 * 1024 * 1024
private const val MAX_CONFLICTS_SHOWN = 20
private const val TAB_CLOSED_GRACE_MS = 1_500L
