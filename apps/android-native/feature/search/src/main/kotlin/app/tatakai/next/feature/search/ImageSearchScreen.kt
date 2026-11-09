// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.search

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.BrokenImage
import androidx.compose.material.icons.outlined.ImageSearch
import androidx.compose.material.icons.outlined.PhotoLibrary
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.ImageMatch
import app.tatakai.next.model.Media
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.formatDuration
import app.tatakai.next.ui.state.screenInsets
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import kotlin.math.roundToInt
import org.koin.compose.viewmodel.koinViewModel

/**
 * Classic `/image-search` (trace.moe): pick an image (SAF `GetContent("image/ *")`, or [imageUri] when the screen was opened by
 * sharing a picture to the app), send its bytes through `MetadataRepository.searchByImage`, show the matches best first (preview
 * frame, title, episode and timestamp, similarity) with a similarity threshold slider; tapping a match opens Details ([onMediaClick]
 * with the `Media` of `ImageMatch.ref`, resolved through `MetadataRepository.media`). Idle / loading / empty / error states.
 */
@Composable
fun ImageSearchScreen(
    onBack: () -> Unit,
    onMediaClick: (Media) -> Unit,
    modifier: Modifier = Modifier,
    /** `content://` or `file://` of a shared image; null = let the user pick one. */
    imageUri: String? = null,
) {
    val viewModel: ImageSearchViewModel = koinViewModel()
    val state by viewModel.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val resources = LocalContext.current.resources
    val currentOnMediaClick by rememberUpdatedState(onMediaClick)
    LaunchedEffect(imageUri) { imageUri?.let(viewModel::search) }
    LaunchedEffect(viewModel) {
        viewModel.events.collect { event ->
            when (event) {
                is ImageSearchEvent.Open -> currentOnMediaClick(event.media)
                is ImageSearchEvent.OpenFailed -> snackbar.show(resources.getString(R.string.search_image_open_failed))
            }
        }
    }
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        if (uri != null) viewModel.search(uri.toString(), force = true)
    }
    val pick = { picker.launch("image/*") }
    Scaffold(
        modifier = modifier,
        contentWindowInsets = screenInsets,
        topBar = {
            Row(
                Modifier.fillMaxWidth().statusBarsPadding().padding(4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                IconButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back))
                }
                Text(stringResource(R.string.search_image_title), style = MaterialTheme.typography.titleLarge, modifier = Modifier.weight(1f))
                IconButton(onClick = pick) {
                    Icon(Icons.Outlined.PhotoLibrary, contentDescription = stringResource(R.string.search_image_pick))
                }
            }
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when (val results = state.results) {
                ImageSearchResults.Idle -> EmptyState(
                    icon = Icons.Outlined.ImageSearch,
                    title = stringResource(R.string.search_image_idle_title),
                    message = stringResource(R.string.search_image_idle_message),
                    actionLabel = stringResource(R.string.search_image_pick),
                    onAction = pick,
                    modifier = Modifier.align(Alignment.Center),
                )
                ImageSearchResults.Loading -> Column(Modifier.fillMaxSize()) {
                    PickedPreview(state.imageUri)
                    if (rememberAfterDelay()) LoadingState()
                }
                ImageSearchResults.Unreadable -> EmptyState(
                    icon = Icons.Outlined.BrokenImage,
                    title = stringResource(R.string.search_image_unreadable_title),
                    message = stringResource(R.string.search_image_unreadable_message),
                    actionLabel = stringResource(R.string.search_image_pick),
                    onAction = pick,
                    modifier = Modifier.align(Alignment.Center),
                )
                ImageSearchResults.TooLarge -> EmptyState(
                    icon = Icons.Outlined.BrokenImage,
                    title = stringResource(R.string.search_image_too_large_title),
                    message = stringResource(R.string.search_image_too_large_message),
                    actionLabel = stringResource(R.string.search_image_pick),
                    onAction = pick,
                    modifier = Modifier.align(Alignment.Center),
                )
                is ImageSearchResults.Err -> ErrorState(
                    message = results.cause.userMessage(),
                    retryLabel = stringResource(UiR.string.ui_retry),
                    onRetry = viewModel::retry,
                    title = stringResource(UiR.string.ui_error_title),
                    modifier = Modifier.align(Alignment.Center),
                )
                is ImageSearchResults.Ok -> MatchList(
                    state = state,
                    onThreshold = viewModel::setThreshold,
                    onOpen = viewModel::open,
                    onPick = pick,
                )
            }
            if (state.opening) {
                LinearProgressIndicator(Modifier.fillMaxWidth().align(Alignment.TopCenter))
            }
        }
    }
}

@Composable
private fun PickedPreview(uri: String?) {
    if (uri == null) return
    AsyncImage(
        model = uri,
        contentDescription = stringResource(R.string.search_image_picked),
        contentScale = ContentScale.Crop,
        modifier = Modifier
            .fillMaxWidth()
            .height(160.dp)
            .padding(horizontal = 16.dp, vertical = 8.dp)
            .clip(MaterialTheme.shapes.large)
            .background(MaterialTheme.colorScheme.surfaceContainerHighest),
    )
}

@Composable
private fun MatchList(
    state: ImageSearchUiState,
    onThreshold: (Float) -> Unit,
    onOpen: (ImageMatch) -> Unit,
    onPick: () -> Unit,
) {
    val shown = state.shown
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "preview") { PickedPreview(state.imageUri) }
        item(key = "threshold") {
            Column(Modifier.padding(horizontal = 16.dp)) {
                Text(
                    stringResource(R.string.search_image_threshold, (state.threshold * 100).roundToInt()),
                    style = MaterialTheme.typography.labelLarge,
                )
                Slider(value = state.threshold, onValueChange = onThreshold, valueRange = 0.5f..1f)
                if (state.hiddenCount > 0) {
                    Text(
                        stringResource(R.string.search_image_hidden, state.hiddenCount),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
        if (shown.isEmpty()) {
            item(key = "empty") {
                EmptyState(
                    icon = Icons.Outlined.SearchOff,
                    title = stringResource(R.string.search_image_empty_title),
                    message = stringResource(R.string.search_image_empty_message),
                    actionLabel = stringResource(R.string.search_image_pick),
                    onAction = onPick,
                )
            }
        }
        items(shown, key = { "${it.ref.anilistId}-${it.episode}-${it.fromSec}" }) { match ->
            MatchRow(match, onClick = { onOpen(match) })
        }
    }
}

@Composable
private fun MatchRow(match: ImageMatch, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AsyncImage(
            model = match.previewImage,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier
                .width(140.dp)
                .aspectRatio(16f / 9f)
                .clip(MaterialTheme.shapes.medium)
                .background(MaterialTheme.colorScheme.surfaceContainerHighest),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(match.title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            val at = formatDuration(match.fromSec.toLong())
            Text(
                match.episode?.let { stringResource(R.string.search_image_episode_at, it, at) } ?: stringResource(R.string.search_image_at, at),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val pct = (match.similarity * 100).roundToInt()
            Pill(
                stringResource(R.string.search_image_similarity, pct),
                tone = when {
                    match.similarity >= 0.95 -> PillTone.OK
                    match.similarity >= 0.85 -> PillTone.WARN
                    else -> PillTone.ERR
                },
            )
        }
    }
}
