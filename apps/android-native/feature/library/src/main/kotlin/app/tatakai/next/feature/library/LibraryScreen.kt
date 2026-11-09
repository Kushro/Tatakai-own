@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.library

import android.text.format.DateUtils
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material.icons.outlined.VideoLibrary
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.Icon
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.feature.manga.MangaLibraryTab
import app.tatakai.next.feature.novel.NovelLibraryTab
import app.tatakai.next.feature.torrents.TorrentsLibraryTab
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.format.formatDuration
import app.tatakai.next.ui.media.MediaCard
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.progressFraction
import app.tatakai.next.ui.media.toResumeArgs
import app.tatakai.next.ui.state.LoadStateContent
import app.tatakai.next.ui.state.screenInsets
import org.koin.compose.viewmodel.koinViewModel

@Composable
fun LibraryScreen(
    onOpenDetails: (anilistId: Int) -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    onExplore: () -> Unit,
    modifier: Modifier = Modifier,
    viewModel: LibraryViewModel = koinViewModel(),
    // --- android wave contract (docs/redesign/12 §3.14): new callbacks appended, wired by :app (worker b) ---
    /** Tab shown first (You › Historial opens HISTORY; notifications / badges open TORRENTS). */
    initialTab: LibraryTab = LibraryTab.CONTINUE,
    /** Torrents tab card -> Route.Torrents(OVERVIEW, torrentId). */
    onOpenTorrent: (torrentId: String) -> Unit = {},
    // --- parity wave A (15 §3.3, worker q) ---
    /** Lists / Favorites empty state "Vincular AniList o MAL" -> Route.SettingsSection(ACCOUNTS). */
    onOpenAccounts: () -> Unit = {},
    // --- parity wave B (16 §3.5 / §3.6, worker s; wired by :app) ---
    /** Manga readlist row -> Route.Details(MediaKind.MANGA, anilistId). */
    onOpenMangaDetails: (anilistId: Int) -> Unit = {},
    /** Novel readlist row -> Route.NovelDetails(anilistId). */
    onOpenNovelDetails: (anilistId: Int) -> Unit = {},
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val resources = LocalContext.current.resources
    LaunchedEffect(viewModel) {
        viewModel.events.collect { event ->
            when (event) {
                LibraryEvent.Synced -> snackbar.show(resources.getString(R.string.library_synced))
                is LibraryEvent.SyncFailed -> snackbar.show(resources.getString(R.string.library_sync_failed))
            }
        }
    }
    LibraryContent(
        state = state,
        initialTab = initialTab,
        actions = LibraryActions(
            onRetry = viewModel::retry,
            onSyncNow = viewModel::syncNow,
            onOpenDetails = onOpenDetails,
            onPlay = onPlay,
            onExplore = onExplore,
            onOpenTorrent = onOpenTorrent,
            onOpenAccounts = onOpenAccounts,
            onOpenMangaDetails = onOpenMangaDetails,
            onOpenNovelDetails = onOpenNovelDetails,
        ),
        modifier = modifier,
    )
}

internal data class LibraryActions(
    val onRetry: () -> Unit = {},
    val onSyncNow: () -> Unit = {},
    val onOpenDetails: (Int) -> Unit = {},
    val onPlay: (PlayerArgs) -> Unit = {},
    val onExplore: () -> Unit = {},
    val onOpenTorrent: (String) -> Unit = {},
    /** parity wave A (worker q) */
    val onOpenAccounts: () -> Unit = {},
    /** parity wave B (worker s) */
    val onOpenMangaDetails: (Int) -> Unit = {},
    val onOpenNovelDetails: (Int) -> Unit = {},
)

@Composable
internal fun LibraryContent(
    state: LibraryUiState,
    initialTab: LibraryTab,
    actions: LibraryActions,
    modifier: Modifier = Modifier,
) {
    // Keyed on initialTab so Route.LibraryTab(tab) re-selects it; the user's own choice survives configuration changes.
    var tab by rememberSaveable(initialTab) { mutableIntStateOf(initialTab.ordinal) }
    Scaffold(
        modifier = modifier,
        contentWindowInsets = screenInsets,
        topBar = {
            // Scaffold's contentWindowInsets only pad the content; the top bar takes the status bar itself.
            Column(Modifier.fillMaxWidth().statusBarsPadding()) {
                LibraryHeader(state, actions.onSyncNow)
                LibraryTabs(selected = LibraryTab.entries[tab], activeDownloads = state.activeDownloads, onSelect = { tab = it.ordinal })
            }
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when (LibraryTab.entries[tab]) {
                LibraryTab.CONTINUE -> ContinueTab(state.continueWatching, actions)
                LibraryTab.HISTORY -> HistoryTab(state.history, actions)
                // parity wave A (15 §3.3, worker q)
                LibraryTab.LISTS -> ListsTab(
                    onOpenDetails = actions.onOpenDetails,
                    onOpenAccounts = actions.onOpenAccounts,
                    modifier = Modifier.fillMaxSize(),
                )
                LibraryTab.FAVORITES -> FavoritesTab(
                    onOpenDetails = actions.onOpenDetails,
                    onOpenAccounts = actions.onOpenAccounts,
                    modifier = Modifier.fillMaxSize(),
                )
                LibraryTab.TORRENTS -> TorrentsLibraryTab(
                    onOpenTorrent = actions.onOpenTorrent,
                    onPlay = actions.onPlay,
                    modifier = Modifier.fillMaxSize(),
                )
                // parity wave B (16 §3.5 / §3.6, worker s): readlists; the same `onOpenDetails` opens Route.Details(MANGA, id) / NovelDetails
                // through `actions.onOpenMangaDetails` / `onOpenNovelDetails` (wired by :app).
                LibraryTab.MANGA -> MangaLibraryTab(
                    onOpenDetails = actions.onOpenMangaDetails,
                    onOpenAccounts = actions.onOpenAccounts,
                    modifier = Modifier.fillMaxSize(),
                )
                LibraryTab.NOVELS -> NovelLibraryTab(
                    onOpenDetails = actions.onOpenNovelDetails,
                    onOpenAccounts = actions.onOpenAccounts,
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }
}

@Composable
private fun LibraryHeader(state: LibraryUiState, onSyncNow: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(stringResource(R.string.library_title), style = MaterialTheme.typography.headlineMedium)
                Pill(stringResource(R.string.library_local), tone = PillTone.PRIMARY)
            }
            Text(
                state.lastSyncedAt?.let { stringResource(R.string.library_synced_at, relativeTime(it)) }
                    ?: stringResource(R.string.library_never_synced),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        if (state.syncing) {
            val label = stringResource(R.string.library_syncing)
            LoadingIndicator(Modifier.size(40.dp).semantics { contentDescription = label })
        } else {
            TextButton(onClick = onSyncNow) {
                Icon(Icons.Outlined.Sync, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.library_sync_now), modifier = Modifier.padding(start = 6.dp))
            }
        }
    }
}

@Composable
private fun LibraryTabs(selected: LibraryTab, activeDownloads: Int, onSelect: (LibraryTab) -> Unit) {
    PrimaryTabRow(selectedTabIndex = selected.ordinal) {
        LibraryTab.entries.forEach { t ->
            Tab(
                selected = t == selected,
                onClick = { onSelect(t) },
                text = {
                    if (t == LibraryTab.TORRENTS && activeDownloads > 0) {
                        val badge = stringResource(R.string.library_torrents_badge, activeDownloads)
                        // Badge beside the label: BadgedBox anchors it over the end of the text and hides the last letters.
                        Row(
                            modifier = Modifier.semantics(mergeDescendants = true) { contentDescription = badge },
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Text(stringResource(t.labelRes()))
                            Badge { Text(activeDownloads.toString()) }
                        }
                    } else {
                        Text(stringResource(t.labelRes()))
                    }
                },
            )
        }
    }
}

private fun LibraryTab.labelRes(): Int = when (this) {
    LibraryTab.CONTINUE -> R.string.library_tab_continue
    LibraryTab.HISTORY -> R.string.library_tab_history
    LibraryTab.LISTS -> R.string.library_tab_lists
    LibraryTab.FAVORITES -> R.string.library_tab_favorites
    LibraryTab.MANGA -> R.string.library_tab_manga
    LibraryTab.NOVELS -> R.string.library_tab_novels
    LibraryTab.TORRENTS -> R.string.library_tab_torrents
}

@Composable
private fun ContinueTab(state: LoadState<List<HistoryEntry>>, actions: LibraryActions) {
    LoadStateContent(state = state, onRetry = actions.onRetry, loading = { DelayedLoading() }) { entries ->
        if (entries.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.VideoLibrary,
                title = stringResource(R.string.library_empty_title),
                message = stringResource(R.string.library_empty_message),
                actionLabel = stringResource(R.string.library_explore),
                onAction = actions.onExplore,
            )
        } else {
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 110.dp),
                modifier = Modifier.fillMaxSize(),
                contentPadding = PaddingValues(16.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items(entries, key = { "c-" + it.episodeId }, contentType = { "entry" }) { entry ->
                    MediaCard(
                        title = entry.title,
                        coverUrl = entry.poster,
                        onClick = { actions.onPlay(entry.toResumeArgs()) },
                        subtitle = if (entry.completed || entry.progressSec <= 0) {
                            stringResource(R.string.library_episode, entry.episode)
                        } else {
                            stringResource(R.string.library_episode_at, entry.episode, formatDuration(entry.progressSec.toLong()))
                        },
                        progress = if (entry.completed) null else entry.progressFraction(),
                    )
                }
            }
        }
    }
}

@Composable
private fun HistoryTab(state: LoadState<List<HistoryEntry>>, actions: LibraryActions) {
    LoadStateContent(state = state, onRetry = actions.onRetry, loading = { DelayedLoading() }) { entries ->
        if (entries.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.History,
                title = stringResource(R.string.library_history_empty_title),
                message = stringResource(R.string.library_history_empty_message),
                actionLabel = stringResource(R.string.library_explore),
                onAction = actions.onExplore,
            )
        } else {
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(vertical = 8.dp)) {
                items(entries, key = { "h-" + it.episodeId + "-" + it.watchedAt.toEpochMilli() }, contentType = { "entry" }) { entry ->
                    HistoryRow(
                        entry = entry,
                        onClick = { entry.anilistId?.let(actions.onOpenDetails) ?: actions.onPlay(entry.toResumeArgs()) },
                    )
                }
            }
        }
    }
}

/** Prototype history `.ep` row: thumbnail, title + Visto pill, "Episodio N · visto hace 2 h", chevron. */
@Composable
private fun HistoryRow(entry: HistoryEntry, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaCover(url = entry.poster, modifier = Modifier.width(56.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    entry.title,
                    style = MaterialTheme.typography.titleSmall,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                if (entry.completed) Pill(stringResource(R.string.library_watched), tone = PillTone.OK)
            }
            Text(
                stringResource(R.string.library_watched_on, entry.episode, relativeTime(entry.watchedAt.toEpochMilli())),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

/** "hace 5 min" / "5 minutes ago" in the app locale (platform formatter). */
@Composable
private fun relativeTime(epochMs: Long): String {
    LocalContext.current // recompose on configuration (locale) changes
    return DateUtils.getRelativeTimeSpanString(epochMs, System.currentTimeMillis(), DateUtils.MINUTE_IN_MILLIS).toString()
}

@Composable
private fun DelayedLoading() {
    if (rememberAfterDelay()) LoadingState()
}
