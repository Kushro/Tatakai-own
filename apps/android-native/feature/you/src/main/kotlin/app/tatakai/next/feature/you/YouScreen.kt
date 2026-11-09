// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.you

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.outlined.Bookmarks
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.ListItem
import androidx.compose.material3.ListItemDefaults
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.LoadState
import app.tatakai.next.ui.format.relativeTime
import app.tatakai.next.ui.labels.labelRes
import app.tatakai.next.ui.state.screenInsets
import app.tatakai.next.ui.state.userMessage
import org.koin.compose.viewmodel.koinViewModel
import app.tatakai.next.ui.R as UiR

// android wave contract (docs/redesign/12 §3.13). Owner: worker b. Name and signature FIXED.

/**
 * "Tú" (prototype android.js `S.you`): account card (avatar, email or guest, last sync, "Sincronizar ahora" / "Iniciar sesión") and the
 * list Historial · Descargas · Extensiones · Dispositivo enlazado · Ajustes with one-line status each.
 */
@Composable
fun YouScreen(
    onOpenHistory: () -> Unit,
    onOpenDownloads: () -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenLinked: () -> Unit,
    onOpenSettings: () -> Unit,
    modifier: Modifier = Modifier,
    // --- parity wave A (docs/redesign/15 §3.3, worker q): wired by :app. 2026-11-09 amendment: the Supabase sign-in callback is
    // gone; the account card's button and the "Listas y cuentas" row both lead here. ---
    /** AniList / MAL accounts -> Route.SettingsSection(ACCOUNTS). */
    onOpenAccounts: () -> Unit = {},
) {
    val viewModel: YouViewModel = koinViewModel()
    val state by viewModel.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val resources = LocalContext.current.resources
    val currentSnackbar by rememberUpdatedState(snackbar)
    LaunchedEffect(viewModel) {
        viewModel.events.collect { event ->
            when (event) {
                YouEvent.Synced -> currentSnackbar.show(resources.getString(R.string.you_synced_now))
                is YouEvent.SyncFailed -> currentSnackbar.show(
                    resources.getString(R.string.you_sync_failed),
                    actionLabel = resources.getString(UiR.string.ui_retry),
                    onAction = viewModel::syncNow,
                )
            }
        }
    }
    YouContent(
        state = state,
        onRetry = viewModel::retry,
        onSyncNow = viewModel::syncNow,
        onOpenHistory = onOpenHistory,
        onOpenDownloads = onOpenDownloads,
        onOpenExtensions = onOpenExtensions,
        onOpenLinked = onOpenLinked,
        onOpenSettings = onOpenSettings,
        onOpenAccounts = onOpenAccounts,
        modifier = modifier,
    )
}

@Composable
internal fun YouContent(
    state: YouUiState,
    onRetry: () -> Unit,
    onSyncNow: () -> Unit,
    onOpenHistory: () -> Unit,
    onOpenDownloads: () -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenLinked: () -> Unit,
    onOpenSettings: () -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Scaffold(modifier = modifier, contentWindowInsets = screenInsets) { padding ->
        Box(Modifier.fillMaxSize().padding(top = padding.calculateTopPadding()), contentAlignment = Alignment.TopCenter) {
            when (val overview = state.overview) {
                LoadState.Loading -> if (rememberAfterDelay(150)) YouSkeleton()
                is LoadState.Err -> ErrorState(
                    message = overview.cause.userMessage(),
                    retryLabel = stringResource(UiR.string.ui_retry),
                    onRetry = onRetry,
                    title = stringResource(R.string.you_load_error),
                    modifier = Modifier.fillMaxSize(),
                )
                is LoadState.Ok -> LazyColumn(
                    modifier = Modifier.fillMaxSize().widthIn(max = 720.dp),
                    contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
                    verticalArrangement = Arrangement.spacedBy(16.dp),
                ) {
                    item(key = "title", contentType = "title") {
                        Text(
                            stringResource(R.string.you_title),
                            style = MaterialTheme.typography.headlineMedium,
                            modifier = Modifier.semantics { heading() },
                        )
                    }
                    item(key = "account", contentType = "account") {
                        AccountCard(overview.value, syncing = state.syncing, onSyncNow = onSyncNow, onOpenAccounts = onOpenAccounts)
                    }
                    item(key = "list", contentType = "list") {
                        YouList(
                            overview.value,
                            onOpenHistory = onOpenHistory,
                            onOpenDownloads = onOpenDownloads,
                            onOpenExtensions = onOpenExtensions,
                            onOpenLinked = onOpenLinked,
                            onOpenSettings = onOpenSettings,
                            onOpenAccounts = onOpenAccounts,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun AccountCard(overview: YouOverview, syncing: Boolean, onSyncNow: () -> Unit, onOpenAccounts: () -> Unit) {
    // 2026-11-09 amendment (no own login): the identity is the linked AniList / MAL account(s); the history syncs with the paired PC.
    val primary = overview.accounts.firstOrNull { it.provider == ListProvider.ANILIST } ?: overview.accounts.firstOrNull()
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        shape = MaterialTheme.shapes.extraLarge,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                Avatar(primary?.username)
                Column(Modifier.weight(1f)) {
                    Text(
                        primary?.username ?: stringResource(R.string.you_guest_name),
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    val subtitle = if (overview.accounts.isEmpty()) {
                        stringResource(R.string.you_guest)
                    } else {
                        val anilist = stringResource(ListProvider.ANILIST.labelRes())
                        val mal = stringResource(ListProvider.MAL.labelRes())
                        overview.accounts.joinToString(" · ") { if (it.provider == ListProvider.ANILIST) anilist else mal }
                    }
                    Text(subtitle, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    val linked = overview.linked
                    val historyLine = when {
                        linked is LinkedSummary.Paired && overview.lastSyncedAt != null ->
                            stringResource(R.string.you_history_link, linked.name, relativeTime(overview.lastSyncedAt))
                        linked is LinkedSummary.Paired -> stringResource(R.string.you_history_link_never, linked.name)
                        else -> stringResource(R.string.you_history_local)
                    }
                    Text(historyLine, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (overview.linked is LinkedSummary.Paired) {
                    FilledTonalButton(onClick = onSyncNow, enabled = !syncing) {
                        if (syncing) {
                            LoadingIndicator(Modifier.size(20.dp))
                        } else {
                            Icon(Icons.Outlined.Sync, contentDescription = null, modifier = Modifier.size(18.dp))
                        }
                        Text(
                            stringResource(if (syncing) R.string.you_syncing else R.string.you_sync_now),
                            modifier = Modifier.padding(start = 8.dp),
                        )
                    }
                }
                FilledTonalButton(onClick = onOpenAccounts) {
                    Icon(Icons.Outlined.Person, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(
                        stringResource(if (overview.accounts.isEmpty()) R.string.you_sign_in else R.string.you_accounts),
                        modifier = Modifier.padding(start = 8.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun Avatar(username: String?) {
    Box(
        Modifier.size(56.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primaryContainer),
        contentAlignment = Alignment.Center,
    ) {
        val initial = username?.firstOrNull { it.isLetterOrDigit() }?.uppercaseChar()
        if (initial != null) {
            Text(initial.toString(), style = MaterialTheme.typography.titleLarge, color = MaterialTheme.colorScheme.onPrimaryContainer)
        } else {
            Icon(Icons.Outlined.Person, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimaryContainer)
        }
    }
}

@Composable
private fun YouList(
    overview: YouOverview,
    onOpenHistory: () -> Unit,
    onOpenDownloads: () -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenLinked: () -> Unit,
    onOpenSettings: () -> Unit,
    onOpenAccounts: () -> Unit,
) {
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = MaterialTheme.shapes.large) {
        Column(Modifier.padding(vertical = 8.dp)) {
            // parity wave A (15 §3.3, worker q): AniList / MAL lists and accounts (Settings › Listas y cuentas).
            YouRow(
                icon = Icons.Outlined.Bookmarks,
                title = stringResource(R.string.you_accounts),
                subtitle = if (overview.accounts.isEmpty()) {
                    stringResource(R.string.you_accounts_none)
                } else {
                    val anilist = stringResource(ListProvider.ANILIST.labelRes())
                    val mal = stringResource(ListProvider.MAL.labelRes())
                    overview.accounts.joinToString(" · ") { "${if (it.provider == ListProvider.ANILIST) anilist else mal} · ${it.username}" }
                },
                onClick = onOpenAccounts,
            )
            YouRow(
                icon = Icons.Outlined.History,
                title = stringResource(R.string.you_history),
                subtitle = if (overview.historyCount == 0) {
                    stringResource(R.string.you_history_empty)
                } else {
                    pluralStringResource(R.plurals.you_history_count, overview.historyCount, overview.historyCount)
                },
                onClick = onOpenHistory,
            )
            YouRow(
                icon = Icons.Outlined.Download,
                title = stringResource(R.string.you_downloads),
                subtitle = when {
                    overview.activeDownloads > 0 ->
                        pluralStringResource(R.plurals.you_downloads_active, overview.activeDownloads, overview.activeDownloads)
                    overview.torrentCount > 0 ->
                        pluralStringResource(R.plurals.you_downloads_total, overview.torrentCount, overview.torrentCount)
                    else -> stringResource(R.string.you_downloads_none)
                },
                badge = overview.activeDownloads.takeIf { it > 0 }?.toString(),
                onClick = onOpenDownloads,
            )
            YouRow(
                icon = Icons.Outlined.Extension,
                title = stringResource(R.string.you_extensions),
                subtitle = if (overview.extensionsCount == 0) {
                    stringResource(R.string.you_extensions_none)
                } else {
                    pluralStringResource(R.plurals.you_extensions_count, overview.extensionsCount, overview.extensionsCount)
                },
                onClick = onOpenExtensions,
            )
            YouRow(
                icon = Icons.Outlined.Computer,
                title = stringResource(R.string.you_linked),
                subtitle = when (val linked = overview.linked) {
                    LinkedSummary.None -> stringResource(R.string.you_linked_none)
                    LinkedSummary.Problem -> stringResource(R.string.you_linked_problem)
                    is LinkedSummary.Paired -> stringResource(
                        if (linked.reachable) R.string.you_linked_connected else R.string.you_linked_unreachable,
                        linked.name,
                    )
                },
                onClick = onOpenLinked,
            )
            YouRow(
                icon = Icons.Outlined.Settings,
                title = stringResource(R.string.you_settings),
                subtitle = stringResource(R.string.you_settings_summary),
                onClick = onOpenSettings,
            )
        }
    }
}

@Composable
private fun YouRow(icon: ImageVector, title: String, subtitle: String, onClick: () -> Unit, badge: String? = null) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Text(subtitle, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        leadingContent = {
            Box(
                Modifier.size(40.dp).clip(CircleShape).background(MaterialTheme.colorScheme.secondaryContainer),
                contentAlignment = Alignment.Center,
            ) {
                Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onSecondaryContainer)
            }
        },
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                if (badge != null) Pill(badge, tone = PillTone.PRIMARY)
                Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null)
            }
        },
        colors = ListItemDefaults.colors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
        modifier = Modifier.clickable(onClick = onClick),
    )
}

@Composable
private fun YouSkeleton() {
    Column(
        Modifier.fillMaxWidth().widthIn(max = 720.dp).padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SkeletonBox(Modifier.fillMaxWidth(0.3f).height(32.dp))
        SkeletonBox(Modifier.fillMaxWidth().height(96.dp), shape = MaterialTheme.shapes.extraLarge)
        repeat(5) { SkeletonBox(Modifier.fillMaxWidth().height(56.dp)) }
    }
}
