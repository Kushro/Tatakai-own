package app.tatakai.next.feature.search

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
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
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.Explore
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.ImageSearch
import androidx.compose.material.icons.outlined.Person
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material3.AssistChip
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
import androidx.compose.material3.TextButton
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
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.feature.details.CharacterScreen
import app.tatakai.next.metadata.TrendWindow
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.alternativeTitle
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.media.mediaKey
import app.tatakai.next.ui.state.screenInsets
import app.tatakai.next.ui.state.userMessage
import java.util.Locale
import org.koin.compose.viewmodel.koinViewModel

/**
 * Search (prototype S.search + classic `SearchPage`): recent searches, Explore feeds, type chips (Anime / Characters), source chips
 * (metadata or one extension with its own catalogue), server-side filters bottom sheet, results counter with infinite scroll, image
 * search entry. On medium/expanded windows the app shell shows Search as the list pane and Details as the detail pane
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
    // --- parity wave A (docs/redesign/15 §3.1, worker o): appended, wired by :app ---
    /** Studio / producer browse from Details (`Route.SearchFor(studio)`): fed into `MediaSearchQuery.studio`. */
    initialStudio: String? = null,
    /** Image-search entry (camera / gallery icon of the search bar) -> Route.ImageSearch(). */
    onOpenImageSearch: () -> Unit = {},
) {
    LaunchedEffect(initialQuery, initialFeed, initialGenre, initialStudio) {
        viewModel.applyInitial(initialQuery, initialFeed, initialGenre, initialStudio)
    }
    // Recent searches may have been cleared from Settings › Metadatos while this screen sat in the back stack.
    LaunchedEffect(Unit) { viewModel.reloadHistory() }
    val state by viewModel.state.collectAsStateWithLifecycle()
    // A character hit opens its page in place (the contract gives Search no character route callback); back returns to the list.
    var openCharacter by rememberSaveable { mutableStateOf<Int?>(null) }
    val characterId = openCharacter
    if (characterId != null) {
        BackHandler { openCharacter = null }
        CharacterScreen(
            characterId = characterId,
            onBack = { openCharacter = null },
            onMediaClick = onMediaClick,
            modifier = modifier,
        )
        return
    }
    SearchContent(
        state = state,
        actions = SearchActions(
            onQueryChange = viewModel::onQueryChange,
            onSubmit = viewModel::submit,
            onClear = viewModel::clear,
            onRetry = viewModel::retry,
            onLoadMore = viewModel::loadMore,
            onMediaClick = onMediaClick,
            onCharacterClick = { openCharacter = it.id },
            onExtensionItemClick = viewModel::openExtensionItem,
            onUseHistory = viewModel::useHistory,
            onRemoveHistory = viewModel::removeHistory,
            onClearHistory = viewModel::clearHistory,
            onFiltersChange = viewModel::setFilters,
            onApplySheet = viewModel::applySheet,
            onOpenSheet = viewModel::ensureTags,
            onSelectFeed = viewModel::selectFeed,
            onClearFeed = viewModel::clearFeed,
            onTrendWindow = viewModel::setTrendWindow,
            onClearStudio = viewModel::clearStudio,
            onType = viewModel::setType,
            onExtension = viewModel::selectExtension,
            onOpenImageSearch = onOpenImageSearch,
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
    val onCharacterClick: (CharacterRef) -> Unit = {},
    val onExtensionItemClick: (ExtensionHomeItem) -> Unit = {},
    val onUseHistory: (String) -> Unit = {},
    val onRemoveHistory: (String) -> Unit = {},
    val onClearHistory: () -> Unit = {},
    val onFiltersChange: (SearchFilters) -> Unit = {},
    val onApplySheet: (SearchFilters, Boolean) -> Unit = { _, _ -> },
    val onOpenSheet: () -> Unit = {},
    val onSelectFeed: (FeedKind) -> Unit = {},
    val onClearFeed: () -> Unit = {},
    val onTrendWindow: (TrendWindow) -> Unit = {},
    val onClearStudio: () -> Unit = {},
    val onType: (SearchType) -> Unit = {},
    val onExtension: (String?) -> Unit = {},
    val onOpenImageSearch: () -> Unit = {},
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
        topBar = {
            SearchTopBar(state, actions, onOpenFilters = {
                actions.onOpenSheet()
                filtersOpen = true
            })
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when (val results = state.results) {
                SearchResults.Idle -> IdleContent(state, actions)
                SearchResults.Loading -> DelayedLoading()
                SearchResults.Empty -> EmptyState(
                    icon = Icons.Outlined.SearchOff,
                    title = stringResource(R.string.search_empty_title),
                    message = if (state.query.isBlank()) {
                        stringResource(R.string.search_empty_filtered)
                    } else {
                        stringResource(R.string.search_empty_message, state.query.trim())
                    },
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
                is SearchResults.Ok -> when {
                    results.characters.isNotEmpty() -> CharacterResults(results, actions)
                    results.extensionItems.isNotEmpty() -> ExtensionResults(results.extensionItems, actions)
                    state.shown.isEmpty() -> EmptyState(
                        icon = Icons.Outlined.SearchOff,
                        title = stringResource(R.string.search_empty_title),
                        message = stringResource(R.string.search_empty_filtered),
                        actionLabel = stringResource(R.string.search_reset),
                        onAction = { actions.onFiltersChange(SearchFilters()) },
                        modifier = Modifier.align(Alignment.Center),
                    )
                    else -> ResultsList(state.shown, results, state.titleLanguage, actions)
                }
            }
        }
    }
    if (filtersOpen) {
        SearchFiltersSheet(
            current = state.filters,
            adult = state.adult,
            genreOptions = (state.genreOptions + state.filters.genres).distinct().sorted(),
            tagOptions = state.tagOptions,
            tagsLoading = state.tagsLoading,
            onApply = { filters, adult ->
                filtersOpen = false
                actions.onApplySheet(filters, adult)
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
            placeholder = {
                Text(stringResource(if (state.type == SearchType.CHARACTERS) R.string.search_hint_characters else R.string.search_hint))
            },
            leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
            trailingIcon = {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    if (state.query.isNotEmpty()) {
                        IconButton(onClick = actions.onClear) {
                            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.search_clear))
                        }
                    }
                    IconButton(onClick = actions.onOpenImageSearch) {
                        Icon(Icons.Outlined.ImageSearch, contentDescription = stringResource(R.string.search_image_open))
                    }
                    if (state.type == SearchType.ANIME && state.extensionId == null) {
                        FiltersButton(state.filters.activeCount, onOpenFilters)
                    }
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
        val feed = state.feed
        if (feed == FeedKind.TRENDING && state.query.isBlank() && state.type == SearchType.ANIME && state.extensionId == null) {
            Segmented(
                options = TrendWindow.entries.map { SegmentedOption(it, stringResource(it.labelRes())) },
                selected = state.trendWindow,
                onSelect = actions.onTrendWindow,
                small = true,
                modifier = Modifier.padding(horizontal = 16.dp).padding(bottom = 8.dp),
            )
        }
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

/** Type chips · source chips · active feed / studio / filter chips (removable), in one horizontally scrolling row. */
@Composable
private fun ChipsRow(state: SearchUiState, actions: SearchActions) {
    val snackbar = LocalAppSnackbar.current
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
                selected = type == state.type,
                onClick = { actions.onType(type) },
                label = { Text(stringResource(type.labelRes())) },
                leadingIcon = if (type == SearchType.CHARACTERS) {
                    { Icon(Icons.Outlined.Person, contentDescription = null, modifier = Modifier.size(18.dp)) }
                } else {
                    null
                },
            )
        }
        if (state.type == SearchType.ANIME) {
            Box(Modifier.width(1.dp).height(24.dp).background(MaterialTheme.colorScheme.outlineVariant))
            FilterChip(
                selected = state.extensionId == null,
                onClick = { actions.onExtension(null) },
                label = { Text(stringResource(R.string.search_source_metadata)) },
                leadingIcon = { Icon(Icons.Outlined.Sync, contentDescription = null, modifier = Modifier.size(18.dp)) },
            )
            state.extensions.forEach { chip ->
                val ext = chip.extension
                if (chip.searchable) {
                    FilterChip(
                        selected = state.extensionId == ext.id,
                        onClick = { actions.onExtension(if (state.extensionId == ext.id) null else ext.id) },
                        label = { Text(ext.name) },
                        leadingIcon = { Icon(Icons.Outlined.Extension, contentDescription = null, modifier = Modifier.size(18.dp)) },
                    )
                } else {
                    // No own catalogue (no custom sources): disabled with its reason (stateDescription + a snackbar on tap).
                    val reason = stringResource(R.string.search_extension_not_searchable, ext.name)
                    Box {
                        FilterChip(
                            selected = false,
                            onClick = {},
                            enabled = false,
                            label = { Text(ext.name) },
                            leadingIcon = { Icon(Icons.Outlined.Extension, contentDescription = null, modifier = Modifier.size(18.dp)) },
                            modifier = Modifier.semantics { stateDescription = reason },
                        )
                        Box(Modifier.matchParentSize().clickable { snackbar.show(reason) })
                    }
                }
            }
        }
        if (state.type != SearchType.ANIME || state.extensionId != null) return@Row
        val feed = state.feed
        if (feed != null && state.query.isBlank()) {
            RemovableChip(stringResource(feed.labelRes()), actions.onClearFeed)
        }
        state.studio?.let { studio ->
            RemovableChip(stringResource(R.string.search_studio_chip, studio), actions.onClearStudio)
        }
        val f = state.filters
        f.genres.forEach { g -> RemovableChip(g) { actions.onFiltersChange(f.copy(genres = f.genres - g)) } }
        f.tags.forEach { t -> RemovableChip("#$t") { actions.onFiltersChange(f.copy(tags = f.tags - t)) } }
        f.formats.forEach { fm -> RemovableChip(formatLabel(fm)) { actions.onFiltersChange(f.copy(formats = f.formats - fm)) } }
        f.year?.let { RemovableChip(it.toString()) { actions.onFiltersChange(f.copy(year = null)) } }
        f.season?.let { RemovableChip(seasonLabel(it)) { actions.onFiltersChange(f.copy(season = null)) } }
        f.status?.let { RemovableChip(statusLabel(it) ?: it) { actions.onFiltersChange(f.copy(status = null)) } }
        if (f.yearFrom != null || f.yearTo != null) {
            RemovableChip(rangeLabel(R.string.search_years_range, f.yearFrom, f.yearTo)) {
                actions.onFiltersChange(f.copy(yearFrom = null, yearTo = null))
            }
        }
        if (f.episodesFrom != null || f.episodesTo != null) {
            RemovableChip(rangeLabel(R.string.search_episodes_range, f.episodesFrom, f.episodesTo)) {
                actions.onFiltersChange(f.copy(episodesFrom = null, episodesTo = null))
            }
        }
        if (f.scoreFrom != null || f.scoreTo != null) {
            RemovableChip(rangeLabel(R.string.search_score_range, f.scoreFrom, f.scoreTo)) {
                actions.onFiltersChange(f.copy(scoreFrom = null, scoreTo = null))
            }
        }
        f.sort?.let { RemovableChip(stringResource(it.labelRes())) { actions.onFiltersChange(f.copy(sort = null)) } }
        if (state.adult) Pill(stringResource(R.string.search_adult_on), tone = PillTone.WARN)
    }
}

@Composable
private fun rangeLabel(res: Int, from: Int?, to: Int?): String =
    stringResource(res, from?.toString() ?: "…", to?.toString() ?: "…")

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

/** Explore feeds of the classic Trending / Collections pages, then the recent searches (remove one or all). */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun IdleContent(state: SearchUiState, actions: SearchActions) {
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        EmptyState(
            icon = if (state.type == SearchType.CHARACTERS) Icons.Outlined.Person else Icons.Outlined.Search,
            title = stringResource(if (state.type == SearchType.CHARACTERS) R.string.search_idle_characters_title else R.string.search_idle_title),
            message = stringResource(
                when {
                    state.type == SearchType.CHARACTERS -> R.string.search_idle_characters_message
                    state.extensionId != null -> R.string.search_idle_extension_message
                    else -> R.string.search_idle_message
                },
            ),
        )
        if (state.type == SearchType.ANIME && state.extensionId == null) {
            Text(
                stringResource(R.string.search_explore),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            )
            FlowRow(
                Modifier.padding(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                EXPLORE_FEEDS.forEach { feed ->
                    AssistChip(
                        onClick = { actions.onSelectFeed(feed) },
                        label = { Text(stringResource(feed.labelRes())) },
                        leadingIcon = { Icon(Icons.Outlined.Explore, contentDescription = null, modifier = Modifier.size(18.dp)) },
                    )
                }
            }
        }
        if (state.history.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(R.string.search_history),
                    style = MaterialTheme.typography.titleMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = actions.onClearHistory) {
                    Icon(Icons.Outlined.DeleteSweep, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.search_history_clear), modifier = Modifier.padding(start = 4.dp))
                }
            }
            state.history.forEach { entry ->
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

@Composable
private fun ResultsCount(count: Int) {
    val resources = LocalContext.current.resources
    Text(
        resources.getQuantityString(R.plurals.search_results, count, count),
        style = MaterialTheme.typography.labelLarge,
        color = MaterialTheme.colorScheme.onSurfaceVariant,
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
    )
}

/** Infinite scroll: reaching the footer loads the next page once per page; after a failure the button retries by hand. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun LoadMoreFooter(results: SearchResults.Ok, onLoadMore: () -> Unit) {
    LaunchedEffect(results.page) { onLoadMore() }
    Box(Modifier.fillMaxWidth().padding(8.dp), contentAlignment = Alignment.Center) {
        if (results.loadingMore) {
            LoadingIndicator(Modifier.size(40.dp))
        } else {
            OutlinedButton(onClick = onLoadMore) { Text(stringResource(R.string.search_load_more)) }
        }
    }
}

@Composable
private fun ResultsList(shown: List<Media>, results: SearchResults.Ok, language: TitleLanguage, actions: SearchActions) {
    LazyColumn(modifier = Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "count", contentType = "count") { ResultsCount(shown.size) }
        items(shown, key = { mediaKey(it) }, contentType = { "media" }) { media ->
            ResultRow(media, language, onClick = { actions.onMediaClick(media) })
        }
        if (results.hasNext) {
            item(key = "load-more", contentType = "load-more") { LoadMoreFooter(results, actions.onLoadMore) }
        }
    }
}

@Composable
private fun CharacterResults(results: SearchResults.Ok, actions: SearchActions) {
    LazyColumn(modifier = Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "count", contentType = "count") { ResultsCount(results.characters.size) }
        items(results.characters, key = { "c-${it.id}" }, contentType = { "character" }) { c ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { actions.onCharacterClick(c) }
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                MediaCover(url = c.image, modifier = Modifier.width(48.dp), shape = CircleShape, aspectRatio = 1f)
                Text(c.name, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
        if (results.hasNext) {
            item(key = "load-more", contentType = "load-more") { LoadMoreFooter(results, actions.onLoadMore) }
        }
    }
}

/** Cards of `custom/search`: tapping one looks its title up in the metadata provider (like the Home extension rows). */
@Composable
private fun ExtensionResults(items: List<ExtensionHomeItem>, actions: SearchActions) {
    LazyColumn(modifier = Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        item(key = "count", contentType = "count") { ResultsCount(items.size) }
        items(items, key = { "x-${it.id}-${it.title}" }, contentType = { "ext" }) { item ->
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { actions.onExtensionItemClick(item) }
                    .padding(horizontal = 16.dp, vertical = 8.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                MediaCover(url = item.poster, modifier = Modifier.width(56.dp))
                Column(Modifier.weight(1f)) {
                    Text(item.title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(
                        stringResource(R.string.search_extension_hint),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

/** Prototype `.result`: cover, title (preferred language), "alt · year · season", score, status and 18+ pills. */
@Composable
private fun ResultRow(media: Media, language: TitleLanguage, onClick: () -> Unit) {
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
            val title = media.displayTitle(language)
            Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            val alt = media.alternativeTitle(title)
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
                if (media.isProbablyAdult()) Pill(stringResource(R.string.search_adult_badge), tone = PillTone.ERR)
            }
        }
    }
}

private val EXPLORE_FEEDS = listOf(
    FeedKind.TRENDING, FeedKind.POPULAR, FeedKind.TOP_RATED, FeedKind.AIRING, FeedKind.SEASONAL, FeedKind.UPCOMING,
    FeedKind.RECENTLY_COMPLETED,
)

internal fun SearchType.labelRes(): Int = when (this) {
    SearchType.ANIME -> R.string.search_type_anime
    SearchType.CHARACTERS -> R.string.search_type_characters
}

internal fun TrendWindow.labelRes(): Int = when (this) {
    TrendWindow.TODAY -> R.string.search_trend_today
    TrendWindow.WEEK -> R.string.search_trend_week
    TrendWindow.MONTH -> R.string.search_trend_month
    TrendWindow.ALL_TIME -> R.string.search_trend_all_time
}

internal fun MediaSort.labelRes(): Int = when (this) {
    MediaSort.POPULARITY -> R.string.search_sort_popularity
    MediaSort.TRENDING -> R.string.search_sort_trending
    MediaSort.SCORE -> R.string.search_sort_score
    MediaSort.START_DATE -> R.string.search_sort_start_date
    MediaSort.TITLE -> R.string.search_sort_title
    MediaSort.FAVOURITES -> R.string.search_sort_favourites
    MediaSort.EPISODES -> R.string.search_sort_episodes
    MediaSort.RELEVANCE -> R.string.search_sort_relevance
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

@Composable
internal fun formatLabel(format: String): String = when (format.uppercase()) {
    "TV" -> stringResource(R.string.search_format_tv)
    "TV_SHORT" -> stringResource(R.string.search_format_tv_short)
    "MOVIE" -> stringResource(R.string.search_format_movie)
    "OVA" -> stringResource(R.string.search_format_ova)
    "ONA" -> stringResource(R.string.search_format_ona)
    "SPECIAL" -> stringResource(R.string.search_format_special)
    "MUSIC" -> stringResource(R.string.search_format_music)
    else -> format
}

internal fun statusTone(status: String): PillTone = when (status.uppercase()) {
    "RELEASING" -> PillTone.OK
    "NOT_YET_RELEASED", "HIATUS" -> PillTone.WARN
    "CANCELLED" -> PillTone.ERR
    else -> PillTone.NEUTRAL
}
