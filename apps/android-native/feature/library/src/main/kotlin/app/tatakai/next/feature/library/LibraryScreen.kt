package app.tatakai.next.feature.library

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.VideoLibrary
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ListGroupHeader
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.MediaCard
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
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    LibraryContent(state, viewModel::retry, onOpenDetails, onPlay, onExplore, modifier)
}

@Composable
internal fun LibraryContent(
    state: LoadState<LibraryData>,
    onRetry: () -> Unit,
    onOpenDetails: (Int) -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    onExplore: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Scaffold(modifier = modifier, contentWindowInsets = screenInsets) { padding ->
        LoadStateContent(state = state, onRetry = onRetry, modifier = Modifier.padding(padding)) { data ->
            if (data.isEmpty) {
                EmptyState(
                    icon = Icons.Outlined.VideoLibrary,
                    title = stringResource(R.string.library_empty_title),
                    message = stringResource(R.string.library_empty_message),
                    actionLabel = stringResource(R.string.library_explore),
                    onAction = onExplore,
                    modifier = Modifier.padding(padding),
                )
            } else {
                LibraryGrid(data, padding, onOpenDetails, onPlay)
            }
        }
    }
}

@Composable
private fun LibraryGrid(
    data: LibraryData,
    padding: PaddingValues,
    onOpenDetails: (Int) -> Unit,
    onPlay: (PlayerArgs) -> Unit,
) {
    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 110.dp),
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = padding.calculateTopPadding(), bottom = 24.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        if (data.inProgress.isNotEmpty()) {
            item(key = "h-progress", span = { GridItemSpan(maxLineSpan) }, contentType = "header") {
                ListGroupHeader(stringResource(R.string.library_in_progress))
            }
            items(data.inProgress, key = { "p-" + it.episodeId }, contentType = { "entry" }) { entry ->
                EntryCard(entry, onClick = { onPlay(entry.toResumeArgs()) })
            }
        }
        item(key = "h-history", span = { GridItemSpan(maxLineSpan) }, contentType = "header") {
            ListGroupHeader(stringResource(R.string.library_history))
        }
        items(data.history, key = { "h-" + it.episodeId }, contentType = { "entry" }) { entry ->
            EntryCard(entry, onClick = { entry.anilistId?.let(onOpenDetails) ?: onPlay(entry.toResumeArgs()) })
        }
    }
}

@Composable
private fun EntryCard(entry: HistoryEntry, onClick: () -> Unit) {
    MediaCard(
        title = entry.title,
        coverUrl = entry.poster,
        onClick = onClick,
        subtitle = stringResource(UiR.string.ui_episode_number, entry.episode),
        progress = if (entry.completed) null else entry.progressFraction(),
    )
}
