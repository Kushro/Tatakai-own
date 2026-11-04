package app.tatakai.next.feature.search

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.Media
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.mediaKey
import app.tatakai.next.ui.state.screenInsets
import app.tatakai.next.ui.state.userMessage
import java.util.Locale
import org.koin.compose.viewmodel.koinViewModel

/**
 * Search (prototype S.search): recent searches, type chips (Manga / Novela disabled until M1), source chips, filters bottom sheet,
 * results counter. On medium/expanded windows the app shell shows Search as the list pane and Details as the detail pane
 * (ListDetailSceneStrategy), which is the list-detail layout of 07 §1.
 */
@Composable
fun SearchScreen(
    onMediaClick: (Media) -> Unit,
    modifier: Modifier = Modifier,
    viewModel: SearchViewModel = koinViewModel(),
    // --- android wave contract (docs/redesign/12 §3.14): new callbacks appended, wired by :app (worker b) ---
    /** Pre-filled query / feed / genre from Route.SearchFor (null = plain Search). */
    initialQuery: String? = null,
    initialFeed: FeedKind? = null,
    initialGenre: String? = null,
) {
    LaunchedEffect(initialQuery, initialFeed, initialGenre) {
        viewModel.applyInitial(initialQuery, initialFeed, initialGenre)
    }
    val state by viewModel.state.collectAsStateWithLifecycle()
    SearchContent(
        state = state,
        actions = SearchActions(
            onQueryChange = viewModel::onQueryChange,
            onSubmit = viewModel::submit,
            onClear = viewModel::clear,
            onRetry = viewModel::retry,
            onLoadMore = viewModel::loadMore,
            onMediaClick = onMediaClick,
            onUseHistory = viewModel::useHistory,
            onRemoveHistory = viewModel::removeHistory,
            onFiltersChange = viewModel::setFilters,
            onClearFeed = viewModel::clearFeed,
        ),
        modifier = modifier,
    )
}

internal data class SearchActions(
    val onQueryChange: (String) -> Unit = {},
    val onSubmit: () -> Unit = {},
    val onClear: () -> Unit = {},
    val onRetry: () -> Unit = {},
    val onLoadMore: () -> Unit = {},
    val onMediaClick: (Media) -> Unit = {},
    val onUseHistory: (String) -> Unit = {},
    val onRemoveHistory: (String) -> Unit = {},
    val onFiltersChange: (SearchFilters) -> Unit = {},
    val onClearFeed: () -> Unit = {},
)

@Composable
internal fun SearchContent(
    state: SearchUiState,
    actions: SearchActions,
    modifier: Modifier = Modifier,
) {
    var filtersOpen by rememberSaveable { mutableStateOf(false) }
    Scaffold(
        modifier = modifier,
        contentWindowInsets = screenInsets,
        topBar = { SearchTopBar(state, actions, onOpenFilters = { filtersOpen = true }) },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when (val results = state.results) {
                SearchResults.Idle -> IdleContent(state.history, actions)
                SearchResults.Loading -> DelayedLoading()
                SearchResults.Empty -> EmptyState(
                    icon = Icons.Outlined.SearchOff,
                    title = stringResource(R.string.search_empty_title),
                    message = stringResource(R.string.search_empty_message, state.query.trim()),
                    actionLabel = if (state.filters.isEmpty) null else stringResource(R.string.search_reset),
                    onAction = { actions.onFiltersChange(SearchFilters()) },
                    modifier = Modifier.align(Alignment.Center),
                )
                is SearchResults.Err -> ErrorState(
                    message = results.cause.userMessage(),
                    retryLabel = stringResource(UiR.string.ui_retry),
                    onRetry = actions.onRetry,
                    modifier = Modifier.align(Alignment.Center),
                    title = stringResource(UiR.string.ui_error_title),
                )
                is SearchResults.Ok -> {
                    val shown = state.shown
                    if (shown.isEmpty()) {
                        EmptyState(
                            icon = Icons.Outlined.SearchOff,
                            title = stringResource(R.string.search_empty_title),
                            message = stringResource(R.string.search_empty_filtered),
                            actionLabel = stringResource(R.string.search_reset),
                            onAction = { actions.onFiltersChange(SearchFilters()) },
                            modifier = Modifier.align(Alignment.Center),
                        )
                    } else {
                        ResultsList(shown, results, actions)
                    }
                }
            }
        }
    }
    if (filtersOpen) {
        val loadedGenres = (state.results as? SearchResults.Ok)?.items?.flatMap { it.genres }.orEmpty()
        SearchFiltersSheet(
            current = state.filters,
            genreOptions = (KNOWN_GENRES + loadedGenres + listOfNotNull(state.filters.genre)).distinct().sorted(),
            onApply = {
                filtersOpen = false
                actions.onFiltersChange(it)
            },
            onDismiss = { filtersOpen = false },
        )
    }
}

@Composable
private fun SearchTopBar(state: SearchUiState, actions: SearchActions, onOpenFilters: () -> Unit) {
    val focusManager = LocalFocusManager.current
    // Scaffold's contentWindowInsets only pad the content; the top bar takes the status bar itself.
    Column(Modifier.fillMaxWidth().statusBarsPadding()) {
        Text(
            stringResource(R.string.search_title),
            style = MaterialTheme.typography.headlineMedium,
            modifier = Modifier.padding(horizontal = 16.dp).padding(top = 8.dp),
        )
        TextField(
            value = state.query,
            onValueChange = actions.onQueryChange,
            singleLine = true,
            placeholder = { Text(stringResource(R.string.search_hint)) },
            leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
            trailingIcon = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (state.query.isNotEmpty()) {
                        IconButton(onClick = actions.onClear) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.search_clear))
                        }
                    }
                    FiltersButton(state.filters.activeCount, onOpenFilters)
                }
            },
            shape = CircleShape,
            colors = TextFieldDefaults.colors(
                focusedIndicatorColor = MaterialTheme.colorScheme.surface.copy(alpha = 0f),
                unfocusedIndicatorColor = MaterialTheme.colorScheme.surface.copy(alpha = 0f),
                disabledIndicatorColor = MaterialTheme.colorScheme.surface.copy(alpha = 0f),
            ),
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = {
                actions.onSubmit()
                focusManager.clearFocus()
            }),
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 16.dp, vertical = 8.dp),
        )
        ChipsRow(state, actions)
    }
}

/** LoadingIndicator only after 150 ms, so fast answers do not flash (02 §5). */
@Composable
private fun DelayedLoading() {
    if (rememberAfterDelay()) LoadingState()
}

@Composable
private fun FiltersButton(active: Int, onClick: () -> Unit) {
    val label = if (active > 0) stringResource(R.string.search_filters_active, active) else stringResource(R.string.search_filters)
    IconButton(onClick = onClick) {
        BadgedBox(badge = { if (active > 0) Badge { Text(active.toString()) } }) {
            Icon(Icons.Outlined.Tune, contentDescription = label)
        }
    }
}

/** Type chips · source chips · active feed / filter chips (removable), in one horizontally scrolling row. */
@Composable
private fun ChipsRow(state: SearchUiState, actions: SearchActions) {
    val m1 = stringResource(UiR.string.ui_m1)
    Row(
        Modifier
            .fillMaxWidth()
            .horizontalScroll(rememberScrollState())
            .padding(horizontal = 16.dp)
            .padding(bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SearchType.entries.forEach { type ->
            FilterChip(
                selected = type == SearchType.ANIME,
                onClick = {},
                enabled = type.enabled,
                label = { Text(stringResource(type.labelRes())) },
                modifier = if (type.enabled) Modifier else Modifier.semantics { stateDescription = m1 },
            )
        }
        Box(Modifier.width(1.dp).height(24.dp).background(MaterialTheme.colorScheme.outlineVariant))
        FilterChip(
            selected = true,
            onClick = {},
            label = { Text(stringResource(R.string.search_source_metadata)) },
            leadingIcon = { Icon(Icons.Outlined.Sync, contentDescription = null, modifier = Modifier.size(18.dp)) },
        )
        state.extensions.forEach { ext ->
            // Searching inside an extension is M1: the chip shows which sources exist but cannot be selected yet.
            FilterChip(
                selected = false,
                onClick = {},
                enabled = false,
                label = { Text(ext.name) },
                leadingIcon = { Icon(Icons.Outlined.Extension, contentDescription = null, modifier = Modifier.size(18.dp)) },
                modifier = Modifier.semantics { stateDescription = m1 },
            )
        }
        val feed = state.feed
        if (feed != null && state.query.isBlank()) {
            RemovableChip(stringResource(feed.labelRes()), actions.onClearFeed)
        }
        val f = state.filters
        f.genre?.let { RemovableChip(it) { actions.onFiltersChange(f.copy(genre = null)) } }
        f.year?.let { RemovableChip(it.toString()) { actions.onFiltersChange(f.copy(year = null)) } }
        f.season?.let { RemovableChip(seasonLabel(it)) { actions.onFiltersChange(f.copy(season = null)) } }
        f.status?.let { RemovableChip(statusLabel(it) ?: it) { actions.onFiltersChange(f.copy(status = null)) } }
    }
}

@Composable
private fun RemovableChip(label: String, onRemove: () -> Unit) {
    val description = stringResource(R.string.search_remove_filter, label)
    FilterChip(
        selected = true,
        onClick = onRemove,
        label = { Text(label) },
        trailingIcon = { Icon(Icons.Filled.Close, contentDescription = null, modifier = Modifier.size(18.dp)) },
        modifier = Modifier.semantics { contentDescription = description },
    )
}

@Composable
private fun IdleContent(history: List<String>, actions: SearchActions) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        EmptyState(
            icon = Icons.Outlined.Search,
            title = stringResource(R.string.search_idle_title),
            message = stringResource(R.string.search_idle_message),
        )
        if (history.isNotEmpty()) {
            Text(
                stringResource(R.string.search_history),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            )
            history.forEach { entry ->
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { actions.onUseHistory(entry) }
                        .padding(start = 16.dp, end = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    Icon(Icons.Outlined.History, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant)
                    Text(entry, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                    IconButton(onClick = { actions.onRemoveHistory(entry) }) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.search_history_remove, entry))
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun ResultsList(shown: List<Media>, results: SearchResults.Ok, actions: SearchActions) {
    val resources = LocalContext.current.resources
    LazyColumn(
        modifier = Modifier.fillMaxSize(),
        contentPadding = PaddingValues(bottom = 24.dp),
    ) {
        item(key = "count", contentType = "count") {
            Text(
                resources.getQuantityString(R.plurals.search_results, shown.size, shown.size),
                style = MaterialTheme.typography.labelLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            )
        }
        items(shown, key = { mediaKey(it) }, contentType = { "media" }) { media ->
            ResultRow(media, onClick = { actions.onMediaClick(media) })
        }
        if (results.hasNext) {
            item(key = "load-more", contentType = "load-more") {
                Box(Modifier.fillMaxWidth().padding(8.dp), contentAlignment = Alignment.Center) {
                    if (results.loadingMore) {
                        LoadingIndicator(Modifier.size(40.dp))
                    } else {
                        OutlinedButton(onClick = actions.onLoadMore) { Text(stringResource(R.string.search_load_more)) }
                    }
                }
            }
        }
    }
}

/** Prototype `.result`: cover, title, "alt · year · season", score and status pills. */
@Composable
private fun ResultRow(media: Media, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaCover(url = media.cover, modifier = Modifier.width(56.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(media.title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            val alt = media.titles["romaji"]?.takeIf { it != media.title } ?: media.titles["native"]
            val sub = listOfNotNull(alt, media.year?.toString(), media.season?.let { seasonLabel(it) }).joinToString(" · ")
            if (sub.isNotEmpty()) {
                Text(
                    sub,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), modifier = Modifier.padding(top = 2.dp)) {
                media.score?.let { Pill(String.format(Locale.ROOT, "%.1f", it / 10.0), tone = PillTone.PRIMARY) }
                media.status?.let { status ->
                    statusLabel(status)?.let { Pill(it, tone = statusTone(status)) }
                }
            }
        }
    }
}

internal fun SearchType.labelRes(): Int = when (this) {
    SearchType.ANIME -> R.string.search_type_anime
    SearchType.MANGA -> R.string.search_type_manga
    SearchType.NOVEL -> R.string.search_type_novel
}

internal fun FeedKind.labelRes(): Int = when (this) {
    FeedKind.TRENDING -> R.string.search_feed_trending
    FeedKind.POPULAR -> R.string.search_feed_popular
    FeedKind.TOP_RATED -> R.string.search_feed_top_rated
    FeedKind.AIRING -> R.string.search_feed_airing
    FeedKind.UPCOMING -> R.string.search_feed_upcoming
    FeedKind.SEASONAL -> R.string.search_feed_seasonal
    FeedKind.RECENTLY_COMPLETED -> R.string.search_feed_completed
}

@Composable
internal fun seasonLabel(season: String): String = when (season.uppercase()) {
    "WINTER" -> stringResource(R.string.search_season_winter)
    "SPRING" -> stringResource(R.string.search_season_spring)
    "SUMMER" -> stringResource(R.string.search_season_summer)
    "FALL" -> stringResource(R.string.search_season_fall)
    else -> season
}

@Composable
internal fun statusLabel(status: String): String? = when (status.uppercase()) {
    "RELEASING" -> stringResource(R.string.search_status_releasing)
    "FINISHED" -> stringResource(R.string.search_status_finished)
    "NOT_YET_RELEASED" -> stringResource(R.string.search_status_upcoming)
    "CANCELLED" -> stringResource(R.string.search_status_cancelled)
    "HIATUS" -> stringResource(R.string.search_status_hiatus)
    else -> null
}

internal fun statusTone(status: String): PillTone = when (status.uppercase()) {
    "RELEASING" -> PillTone.OK
    "NOT_YET_RELEASED", "HIATUS" -> PillTone.WARN
    "CANCELLED" -> PillTone.ERR
    else -> PillTone.NEUTRAL
}

/** Provider genre vocabulary (AniList genre names, the same data strings Media.genres carries; shown untranslated like them). */
internal val KNOWN_GENRES = listOf(
    "Action", "Adventure", "Comedy", "Drama", "Ecchi", "Fantasy", "Horror", "Mahou Shoujo", "Mecha", "Music", "Mystery",
    "Psychological", "Romance", "Sci-Fi", "Slice of Life", "Sports", "Supernatural", "Thriller",
)
