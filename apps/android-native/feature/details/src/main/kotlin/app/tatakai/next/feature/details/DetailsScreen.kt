@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.details

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
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
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
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.TatakaiFloatingToolbar
import app.tatakai.next.designsystem.theme.TatakaiSeedTheme
import app.tatakai.next.designsystem.theme.parseHexColor
import app.tatakai.next.model.Episode
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.StreamSource
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.EpisodeRow
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.progressFraction
import app.tatakai.next.ui.state.LoadStateContent
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf

private const val RANGE_SIZE = 100

@Composable
fun DetailsScreen(
    kind: MediaKind,
    anilistId: Int,
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    onOpenExtensions: () -> Unit,
    modifier: Modifier = Modifier,
    viewModel: DetailsViewModel = koinViewModel(
        key = "details-$kind-$anilistId",
        parameters = { parametersOf(kind, MediaRef(anilistId = anilistId)) },
    ),
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val currentOnPlay by rememberUpdatedState(onPlay)
    val currentOnExtensions by rememberUpdatedState(onOpenExtensions)
    LaunchedEffect(viewModel) {
        viewModel.events.collect { event ->
            when (event) {
                is DetailsEvent.NavigateToPlayer -> currentOnPlay(event.args)
                DetailsEvent.OpenExtensions -> currentOnExtensions()
            }
        }
    }
    val seed = parseHexColor((state.media as? LoadState.Ok)?.value?.color)
    TatakaiSeedTheme(seed = seed) {
        DetailsContent(
            state = state,
            onBack = onBack,
            onRetry = viewModel::retry,
            onEpisodeClick = viewModel::onEpisodeClick,
            onContinue = viewModel::onContinueClick,
            onDismissSheet = viewModel::dismissSheet,
            onPlay = viewModel::onPlay,
            onPlayOnPc = viewModel::onPlayOnPc,
            onOpenExtensions = viewModel::onOpenExtensions,
            modifier = modifier,
        )
    }
}

@Composable
internal fun DetailsContent(
    state: DetailsUiState,
    onBack: () -> Unit,
    onRetry: () -> Unit,
    onEpisodeClick: (Int) -> Unit,
    onContinue: () -> Unit,
    onDismissSheet: () -> Unit,
    onPlay: () -> Unit,
    onPlayOnPc: (StreamSource) -> Unit,
    onOpenExtensions: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Box(modifier.fillMaxSize().background(MaterialTheme.colorScheme.background)) {
        LoadStateContent(state = state.media, onRetry = onRetry) { media ->
            MediaBody(media, state, onEpisodeClick)
        }
        IconButton(onClick = onBack, modifier = Modifier.statusBarsPadding().padding(4.dp)) {
            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
        }
        if (state.media is LoadState.Ok) {
            TatakaiFloatingToolbar(
                modifier = Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(16.dp),
            ) {
                TextButton(onClick = onContinue) {
                    Icon(Icons.Filled.PlayArrow, contentDescription = null)
                    Text(
                        if (state.progress.isEmpty()) {
                            stringResource(R.string.details_watch)
                        } else {
                            stringResource(R.string.details_continue, state.continueEpisode)
                        },
                    )
                }
            }
        }
    }
    state.sheet?.let { sheet ->
        AdaptiveSheet(onDismissRequest = onDismissSheet) {
            SourceSheetContent(sheet, state.pcAvailable, onPlay, onPlayOnPc, onOpenExtensions)
        }
    }
}

@Composable
private fun MediaBody(media: Media, state: DetailsUiState, onEpisodeClick: (Int) -> Unit) {
    var range by rememberSaveable { mutableStateOf(0) }
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(bottom = 96.dp),
    ) {
        item(key = "header", contentType = "header") { Header(media) }
        item(key = "episodes-title", contentType = "title") {
            Text(
                stringResource(R.string.details_episodes),
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
            )
        }
        when (val eps = state.episodes) {
            LoadState.Loading -> item(key = "episodes-loading", contentType = "loading") {
                Box(Modifier.fillMaxWidth().height(120.dp)) { LoadingState() }
            }
            is LoadState.Err -> item(key = "episodes-error", contentType = "error") {
                Text(
                    eps.cause.userMessage(),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
            }
            is LoadState.Ok -> {
                val all = eps.value
                if (all.isEmpty()) {
                    item(key = "episodes-empty", contentType = "empty") {
                        Text(
                            stringResource(R.string.details_no_episodes),
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.padding(horizontal = 16.dp),
                        )
                    }
                } else {
                    val ranges = (all.size + RANGE_SIZE - 1) / RANGE_SIZE
                    val selected = range.coerceIn(0, ranges - 1)
                    if (ranges > 1) {
                        item(key = "ranges", contentType = "ranges") {
                            Row(
                                Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                repeat(ranges) { i ->
                                    val from = all[i * RANGE_SIZE].number
                                    val to = all[minOf((i + 1) * RANGE_SIZE, all.size) - 1].number
                                    FilterChip(
                                        selected = i == selected,
                                        onClick = { range = i },
                                        label = { Text("$from–$to") },
                                    )
                                }
                            }
                        }
                    }
                    val page = all.drop(selected * RANGE_SIZE).take(RANGE_SIZE)
                    items(page, key = { it.number }, contentType = { "episode" }) { ep ->
                        EpisodeItem(ep, state.progress[ep.number], onEpisodeClick)
                    }
                }
            }
        }
    }
}

@Composable
private fun EpisodeItem(ep: Episode, progress: HistoryEntry?, onClick: (Int) -> Unit) {
    EpisodeRow(
        number = ep.number,
        title = ep.title,
        imageUrl = ep.image,
        onClick = { onClick(ep.number) },
        airDate = ep.airDate,
        progress = progress?.progressFraction(),
        completed = progress?.completed == true,
        isFiller = ep.isFiller,
    )
}

@Composable
private fun Header(media: Media) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Column(Modifier.fillMaxWidth()) {
        Box(Modifier.fillMaxWidth().height(240.dp)) {
            AsyncImage(
                model = media.banner ?: media.cover,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHighest),
            )
            Box(
                Modifier.fillMaxSize().background(
                    Brush.verticalGradient(listOf(Color.Transparent, MaterialTheme.colorScheme.background)),
                ),
            )
        }
        Row(
            Modifier.padding(horizontal = 16.dp).padding(top = 0.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            MediaCover(url = media.cover, modifier = Modifier.width(110.dp))
            Column(Modifier.weight(1f)) {
                Text(media.title, style = MaterialTheme.typography.headlineSmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
                val meta = listOfNotNull(media.year?.toString(), media.status, media.episodes?.let { "$it ep" })
                if (meta.isNotEmpty()) {
                    Text(
                        meta.joinToString(" • "),
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        if (media.genres.isNotEmpty()) {
            Row(
                Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                media.genres.forEach { InfoChip(it) }
            }
        }
        media.synopsis?.takeIf { it.isNotBlank() }?.let { synopsis ->
            Text(
                synopsis,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = if (expanded) Int.MAX_VALUE else 4,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier
                    .clickable { expanded = !expanded }
                    .padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }
    }
}

@Composable
private fun SourceSheetContent(
    sheet: SourceSheet,
    pcAvailable: Boolean,
    onPlay: () -> Unit,
    onPlayOnPc: (StreamSource) -> Unit,
    onOpenExtensions: () -> Unit,
) {
    Text(
        stringResource(R.string.details_sources_title, sheet.episode),
        style = MaterialTheme.typography.titleLarge,
        modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
    )
    when (val sources = sheet.sources) {
        LoadState.Loading -> Box(Modifier.fillMaxWidth().height(120.dp)) { LoadingState() }
        is LoadState.Err -> Text(
            sources.cause.userMessage(),
            color = MaterialTheme.colorScheme.error,
            modifier = Modifier.padding(horizontal = 24.dp),
        )
        is LoadState.Ok -> if (sources.value.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.Extension,
                title = stringResource(R.string.details_no_sources_title),
                message = stringResource(R.string.details_no_sources_message),
                actionLabel = stringResource(R.string.details_open_extensions),
                onAction = onOpenExtensions,
            )
        } else {
            sources.value.forEachIndexed { index, source ->
                ListItem(
                    headlineContent = { Text(source.provider) },
                    supportingContent = {
                        Text(listOfNotNull(source.quality, source.container.name).joinToString(" • "))
                    },
                    trailingContent = {
                        if (pcAvailable) {
                            AssistChip(
                                onClick = { onPlayOnPc(source) },
                                enabled = !sheet.startingRemote,
                                label = { Text(stringResource(R.string.details_enhance_on_pc)) },
                                leadingIcon = { Icon(Icons.Outlined.Computer, contentDescription = null) },
                            )
                        }
                    },
                    modifier = Modifier.clickable(onClick = onPlay),
                )
                if (index < sources.value.lastIndex) HorizontalDivider()
            }
            sheet.remoteError?.let {
                Text(
                    it.userMessage(),
                    color = MaterialTheme.colorScheme.error,
                    modifier = Modifier.padding(horizontal = 24.dp, vertical = 8.dp),
                )
            }
        }
    }
}
