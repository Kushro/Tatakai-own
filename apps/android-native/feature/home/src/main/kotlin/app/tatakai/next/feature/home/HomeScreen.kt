package app.tatakai.next.feature.home

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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.carousel.HorizontalMultiBrowseCarousel
import androidx.compose.material3.carousel.rememberCarouselState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.ListGroupHeader
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.COVER_ASPECT_RATIO
import app.tatakai.next.ui.media.MediaCard
import app.tatakai.next.ui.media.mediaKey
import app.tatakai.next.ui.media.progressFraction
import app.tatakai.next.ui.media.toResumeArgs
import app.tatakai.next.ui.state.screenInsets
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import org.koin.compose.viewmodel.koinViewModel

@Composable
fun HomeScreen(
    onMediaClick: (Media) -> Unit,
    onSearchClick: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
    viewModel: HomeViewModel = koinViewModel(),
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    HomeContent(
        state = state,
        onMediaClick = onMediaClick,
        onSearchClick = onSearchClick,
        onContinueClick = { onPlay(it.toResumeArgs()) },
        onRetry = viewModel::retry,
        modifier = modifier,
    )
}

@Composable
internal fun HomeContent(
    state: HomeUiState,
    onMediaClick: (Media) -> Unit,
    onSearchClick: () -> Unit,
    onContinueClick: (HistoryEntry) -> Unit,
    onRetry: (FeedKind) -> Unit,
    modifier: Modifier = Modifier,
) {
    Scaffold(modifier = modifier, contentWindowInsets = screenInsets) { padding ->
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(
                top = padding.calculateTopPadding(),
                bottom = 24.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item(key = "search", contentType = "search") {
                SearchEntry(onClick = onSearchClick)
            }
            item(key = "hero", contentType = "hero") {
                HeroSection(state.hero, onMediaClick, onRetry = { onRetry(HERO_FEED) })
            }
            if (state.continueWatching.isNotEmpty()) {
                item(key = "continue", contentType = "row") {
                    ContinueWatchingRow(state.continueWatching, onContinueClick)
                }
            }
            items(state.rows, key = { it.kind.name }, contentType = { "row" }) { row ->
                FeedRowSection(row, onMediaClick, onRetry = { onRetry(row.kind) })
            }
        }
    }
}

@Composable
private fun SearchEntry(onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        shape = CircleShape,
        color = MaterialTheme.colorScheme.surfaceContainerHigh,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp).height(56.dp),
    ) {
        Row(
            Modifier.padding(horizontal = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(Icons.Outlined.Search, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(
                stringResource(R.string.home_search_hint),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun HeroSection(hero: LoadState<List<Media>>, onMediaClick: (Media) -> Unit, onRetry: () -> Unit) {
    when (hero) {
        LoadState.Loading -> if (rememberAfterDelay()) {
            SkeletonBox(Modifier.fillMaxWidth().height(HERO_HEIGHT).padding(horizontal = 16.dp), MaterialTheme.shapes.extraLarge)
        } else {
            Box(Modifier.fillMaxWidth().height(HERO_HEIGHT))
        }
        is LoadState.Err -> ErrorState(
            message = hero.cause.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = onRetry,
            compact = true,
        )
        is LoadState.Ok -> if (hero.value.isNotEmpty()) {
            val items = hero.value
            val carouselState = rememberCarouselState { items.size }
            HorizontalMultiBrowseCarousel(
                state = carouselState,
                preferredItemWidth = 300.dp,
                itemSpacing = 8.dp,
                contentPadding = PaddingValues(horizontal = 16.dp),
                modifier = Modifier.fillMaxWidth().height(HERO_HEIGHT),
            ) { index ->
                val media = items[index]
                Box(
                    Modifier
                        .fillMaxSize()
                        .maskClip(MaterialTheme.shapes.extraLarge)
                        .clickable { onMediaClick(media) },
                ) {
                    AsyncImage(
                        model = media.banner ?: media.cover,
                        contentDescription = null,
                        contentScale = ContentScale.Crop,
                        modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHighest),
                    )
                    Box(
                        Modifier
                            .fillMaxSize()
                            .background(Brush.verticalGradient(listOf(Color.Transparent, Color.Black.copy(alpha = 0.75f)))),
                    )
                    Text(
                        media.title,
                        style = MaterialTheme.typography.titleLarge,
                        color = Color.White,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.align(Alignment.BottomStart).padding(16.dp),
                    )
                }
            }
        }
    }
}

@Composable
private fun ContinueWatchingRow(entries: List<HistoryEntry>, onClick: (HistoryEntry) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        ListGroupHeader(stringResource(R.string.home_continue_watching))
        LazyRow(
            contentPadding = PaddingValues(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            items(entries, key = { it.episodeId }, contentType = { "continue" }) { entry ->
                MediaCard(
                    title = entry.title,
                    coverUrl = entry.poster,
                    onClick = { onClick(entry) },
                    modifier = Modifier.width(CARD_WIDTH),
                    subtitle = stringResource(UiR.string.ui_episode_number, entry.episode),
                    progress = entry.progressFraction(),
                )
            }
        }
    }
}

@Composable
private fun FeedRowSection(row: FeedRow, onMediaClick: (Media) -> Unit, onRetry: () -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        ListGroupHeader(stringResource(row.kind.labelRes()))
        when (val s = row.state) {
            LoadState.Loading -> if (rememberAfterDelay()) {
                LazyRow(
                    contentPadding = PaddingValues(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    userScrollEnabled = false,
                ) {
                    items(SKELETON_COUNT, key = { it }, contentType = { "skeleton" }) {
                        SkeletonBox(Modifier.width(CARD_WIDTH).posterHeight())
                    }
                }
            }
            is LoadState.Err -> ErrorState(
                message = s.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = onRetry,
                compact = true,
            )
            is LoadState.Ok -> LazyRow(
                contentPadding = PaddingValues(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                items(s.value, key = { mediaKey(it) }, contentType = { "media" }) { media ->
                    MediaCard(media = media, onClick = { onMediaClick(media) }, modifier = Modifier.width(CARD_WIDTH))
                }
            }
        }
    }
}

private fun Modifier.posterHeight(): Modifier = this.then(Modifier.height(CARD_WIDTH / COVER_ASPECT_RATIO))

private fun FeedKind.labelRes(): Int = when (this) {
    FeedKind.TRENDING -> R.string.home_feed_trending
    FeedKind.POPULAR -> R.string.home_feed_popular
    FeedKind.TOP_RATED -> R.string.home_feed_top_rated
    FeedKind.AIRING -> R.string.home_feed_airing
    FeedKind.UPCOMING -> R.string.home_feed_upcoming
    FeedKind.SEASONAL -> R.string.home_feed_seasonal
    FeedKind.RECENTLY_COMPLETED -> R.string.home_feed_completed
}

private val HERO_HEIGHT = 220.dp
private val CARD_WIDTH = 120.dp
private const val SKELETON_COUNT = 6
