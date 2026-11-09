package app.tatakai.next.feature.home

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
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.outlined.Bolt
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.TravelExplore
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.HeroCarousel
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.designsystem.components.ListGroupHeader
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.designsystem.theme.LocalReduceMotion
import app.tatakai.next.model.ExtensionHomeRow
import app.tatakai.next.model.FeedKind
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.COVER_ASPECT_RATIO
import app.tatakai.next.ui.media.LocalTitleLanguage
import app.tatakai.next.ui.media.MediaCard
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.media.mediaKey
import app.tatakai.next.ui.media.progressFraction
import app.tatakai.next.ui.media.toResumeArgs
import app.tatakai.next.ui.state.screenInsets
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import java.time.LocalTime
import org.koin.compose.viewmodel.koinViewModel

@Composable
fun HomeScreen(
    onMediaClick: (Media) -> Unit,
    onSearchClick: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
    viewModel: HomeViewModel = koinViewModel(),
    // --- android wave contract (docs/redesign/12 §3.14): new callbacks appended, wired by :app (worker b) ---
    /** Row header "Ver todo" of a feed row -> Route.SearchFor(feed = kind). */
    onOpenFeed: (FeedKind) -> Unit = {},
    /** Genre chip -> Route.SearchFor(genre = genre). */
    onOpenGenre: (String) -> Unit = {},
    /** Item of a lazy extension row -> Route.SearchFor(query = title). */
    onSearchQuery: (String) -> Unit = {},
    /** "Continuar viendo › Ver todo" -> Library (Continuar tab). */
    onOpenLibrary: () -> Unit = {},
    // --- parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1 / §3.4, worker r): appended, wired by :app ---
    /** Header of the "Novedades de lo que sigues" / airing row -> Route.Calendar. */
    onOpenCalendar: () -> Unit = {},
    /** Header of the "Para ti" row -> Route.ForYou. */
    onOpenForYou: () -> Unit = {},
    /** Quick chips Manga / Novelas of the greeting (phones: the only way to the readers besides Search / You) -> Route.Manga() / Route.Novels(). */
    onOpenManga: () -> Unit = {},
    onOpenNovels: () -> Unit = {},
    /** Bell of the top bar (badge = SocialRepository.unreadCount, read with koinInject by r) -> Route.Notifications. */
    onOpenNotifications: () -> Unit = {},
    /** Header of a custom-provider row (`ExtensionHomeRow.extensionId / sourceId`) -> Route.Provider(extensionId, sourceId). */
    onOpenProvider: (extensionId: String, sourceId: String) -> Unit = { _, _ -> },
    /** Item of a custom-provider row -> Route.ProviderItem(extensionId, sourceId, itemId) (replaces the title search of wave android). */
    onOpenProviderItem: (extensionId: String, sourceId: String, itemId: String) -> Unit = { _, _, _ -> },
    /** Continue watching card of a provider title (`ext:` history id) -> resume it in the player. */
    onResumeProviderItem: ((HistoryEntry) -> Unit)? = null,
) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    HomeContent(
        state = state,
        actions = HomeActions(
            onMediaClick = onMediaClick,
            onSearchClick = onSearchClick,
            onWatch = { media -> onPlay(PlayerArgs(media = media.ref, title = media.title, episode = 1)) },
            onContinueClick = { onPlay(it.toResumeArgs()) },
            onRetry = viewModel::retry,
            onOpenFeed = onOpenFeed,
            onOpenGenre = onOpenGenre,
            onSearchQuery = onSearchQuery,
            onOpenLibrary = onOpenLibrary,
            onLoadExtensionRows = viewModel::loadExtensionRows,
            onOpenCalendar = {
                viewModel.markFollowedSeen()
                onOpenCalendar()
            },
            onOpenForYou = onOpenForYou,
            onOpenManga = onOpenManga,
            onOpenNovels = onOpenNovels,
            onOpenNotifications = onOpenNotifications,
            onOpenProvider = onOpenProvider,
            onOpenProviderItem = onOpenProviderItem,
            onResumeProviderItem = onResumeProviderItem,
            onTopWindow = viewModel::loadTop10,
            onRetryLatest = viewModel::loadLatest,
            onRetryUpcoming = viewModel::loadUpcoming,
            onRetryFavourites = viewModel::loadFavourites,
            onRetryFollowed = viewModel::loadFollowed,
            onRetryForYou = viewModel::loadForYou,
            onRetryBecause = viewModel::retryBecause,
            onNextGenreRow = viewModel::loadNextGenreRow,
            onRetryGenre = viewModel::retryGenre,
        ),
        modifier = modifier,
        unreadNotifications = rememberUnreadNotifications(),
    )
}

/** Every callback of the Home content, grouped so previews / tests can pass no-ops. */
internal data class HomeActions(
    val onMediaClick: (Media) -> Unit = {},
    val onSearchClick: () -> Unit = {},
    val onWatch: (Media) -> Unit = {},
    val onContinueClick: (HistoryEntry) -> Unit = {},
    val onRetry: (FeedKind) -> Unit = {},
    val onOpenFeed: (FeedKind) -> Unit = {},
    val onOpenGenre: (String) -> Unit = {},
    val onSearchQuery: (String) -> Unit = {},
    val onOpenLibrary: () -> Unit = {},
    val onLoadExtensionRows: () -> Unit = {},
    // parity wave B (16 §3.1)
    val onOpenCalendar: () -> Unit = {},
    val onOpenForYou: () -> Unit = {},
    val onOpenManga: () -> Unit = {},
    val onOpenNovels: () -> Unit = {},
    val onOpenNotifications: () -> Unit = {},
    val onOpenProvider: (String, String) -> Unit = { _, _ -> },
    val onOpenProviderItem: (String, String, String) -> Unit = { _, _, _ -> },
    val onResumeProviderItem: ((HistoryEntry) -> Unit)? = null,
    val onTopWindow: (TopWindow) -> Unit = {},
    val onRetryLatest: () -> Unit = {},
    val onRetryUpcoming: () -> Unit = {},
    val onRetryFavourites: () -> Unit = {},
    val onRetryFollowed: () -> Unit = {},
    val onRetryForYou: () -> Unit = {},
    val onRetryBecause: () -> Unit = {},
    val onNextGenreRow: (List<String>) -> Unit = {},
    val onRetryGenre: (String) -> Unit = {},
)

@Composable
internal fun HomeContent(
    state: HomeUiState,
    actions: HomeActions,
    modifier: Modifier = Modifier,
    unreadNotifications: Int = 0,
) {
    val gate = rememberMatureGate(state.discover.matureContent)
    val discover = state.discover
    Scaffold(modifier = modifier, contentWindowInsets = screenInsets) { padding ->
        LazyColumn(
            modifier = Modifier.fillMaxSize(),
            contentPadding = PaddingValues(
                top = padding.calculateTopPadding(),
                bottom = 24.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            item(key = "greeting", contentType = "greeting") {
                Greeting(state.providers, unreadNotifications, actions)
            }
            item(key = "search", contentType = "search") {
                SearchEntry(onClick = actions.onSearchClick)
            }
            if (state.isEmpty) {
                item(key = "empty", contentType = "empty") {
                    EmptyState(
                        icon = Icons.Outlined.TravelExplore,
                        title = stringResource(R.string.home_empty_title),
                        message = stringResource(R.string.home_empty_body),
                    )
                }
                return@LazyColumn
            }
            item(key = "hero", contentType = "hero") {
                HeroSection(
                    hero = state.hero,
                    onMediaClick = actions.onMediaClick,
                    onWatch = actions.onWatch,
                    onRetry = { actions.onRetry(HERO_FEED) },
                )
            }
            if (state.continueWatching.isNotEmpty()) {
                item(key = "continue", contentType = "continue") {
                    ContinueWatchingRow(
                        state.continueWatching,
                        onClick = { entry ->
                            val provider = HomeLogic.parseProviderHistoryId(entry.animeId)
                            if (provider != null) {
                                actions.onResumeProviderItem?.invoke(entry)
                                    ?: actions.onOpenProviderItem(provider.first, provider.second, provider.third)
                            } else {
                                actions.onContinueClick(entry)
                            }
                        },
                        onSeeAll = actions.onOpenLibrary,
                    )
                }
            }
            discover.activity?.takeIf { it.total > 0 }?.let { activity ->
                item(key = "downloading", contentType = "downloading") { DownloadingRow(activity) }
            }
            homeDiscoverTop(discover, gate, actions)
            val genres = discover.genreCloud.ifEmpty { state.genres }
            if (genres.isNotEmpty()) {
                item(key = "genres", contentType = "genres") {
                    GenreChips(genres, actions.onOpenGenre)
                }
            }
            items(state.rows, key = { it.kind.name }, contentType = { "row" }) { row ->
                FeedRowSection(
                    row = row,
                    gate = gate,
                    onMediaClick = actions.onMediaClick,
                    onRetry = { actions.onRetry(row.kind) },
                    onSeeAll = { actions.onOpenFeed(row.kind) },
                )
            }
            homeDiscoverBottom(discover, gate, actions)
            item(key = "extension-rows", contentType = "extension-rows") {
                // Composed only when scrolled into view: the lazy request of customHomeRows() (07 §1, prototype IntersectionObserver).
                LaunchedEffect(Unit) { actions.onLoadExtensionRows() }
                ExtensionRows(
                    state.extensionRows,
                    names = discover.extensionNames,
                    onRetry = actions.onLoadExtensionRows,
                    onOpenProvider = actions.onOpenProvider,
                    onItemClick = actions.onOpenProviderItem,
                )
            }
            homeGenreRows(discover, gate, actions, fallbackGenres = genres)
        }
    }
}

@Composable
private fun Greeting(providers: List<String>, unread: Int, actions: HomeActions) {
    val hour = remember { LocalTime.now().hour }
    val greeting = stringResource(
        when {
            hour < 12 -> R.string.home_greeting_morning
            hour < 19 -> R.string.home_greeting_afternoon
            else -> R.string.home_greeting_evening
        },
    )
    val sourcesLabel = stringResource(R.string.home_sources)
    Column(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(greeting, style = MaterialTheme.typography.headlineMedium, modifier = Modifier.weight(1f))
            NotificationBell(unread, actions.onOpenNotifications)
        }
        QuickChips(actions)
        if (providers.isNotEmpty()) {
            Row(
                Modifier.horizontalScroll(rememberScrollState()).semantics { contentDescription = sourcesLabel },
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                providers.forEachIndexed { index, id ->
                    InfoChip(providerLabel(id), icon = if (index == 0) Icons.Filled.Check else null)
                }
            }
        }
    }
}

@Composable
private fun providerLabel(id: String): String = when (id) {
    "anilist" -> stringResource(R.string.home_provider_anilist)
    "kitsu" -> stringResource(R.string.home_provider_kitsu)
    "anizip" -> stringResource(R.string.home_provider_anizip)
    "mal" -> stringResource(R.string.home_provider_mal)
    "shikimori" -> stringResource(R.string.home_provider_shikimori)
    "tenrai" -> stringResource(R.string.home_provider_tenrai)
    else -> id
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

@Composable
private fun HeroSection(
    hero: LoadState<List<Media>>,
    onMediaClick: (Media) -> Unit,
    onWatch: (Media) -> Unit,
    onRetry: () -> Unit,
) {
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
            HeroCarousel(
                items = hero.value,
                key = { mediaKey(it) },
                modifier = Modifier.fillMaxWidth().height(HERO_HEIGHT),
                // HeroCarousel already turns autoplay off with LocalReduceMotion; passing it here keeps the intent explicit.
                autoplay = !LocalReduceMotion.current,
                autoplayMillis = HERO_AUTOPLAY_MS,
            ) { media, index ->
                HeroSlide(media, index, onMediaClick, onWatch)
            }
        }
    }
}

@Composable
private fun HeroSlide(media: Media, index: Int, onMediaClick: (Media) -> Unit, onWatch: (Media) -> Unit) {
    val surface = MaterialTheme.colorScheme.surface
    Box(
        Modifier
            .fillMaxSize()
            .clip(MaterialTheme.shapes.extraLarge)
            .background(MaterialTheme.colorScheme.surfaceContainerHighest)
            .clickable { onMediaClick(media) },
    ) {
        AsyncImage(
            model = media.banner ?: media.cover,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.fillMaxSize(),
        )
        Box(
            Modifier
                .fillMaxSize()
                .background(Brush.verticalGradient(listOf(surface.copy(alpha = 0f), surface.copy(alpha = 0.94f)))),
        )
        Column(
            Modifier.align(Alignment.BottomStart).padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Icon(Icons.Outlined.Bolt, contentDescription = null, modifier = Modifier.size(16.dp), tint = MaterialTheme.colorScheme.primary)
                Text(
                    stringResource(R.string.home_hero_kicker, index + 1),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                )
                statusLabel(media.status)?.let { (label, tone) -> Pill(label, tone = tone) }
            }
            Text(
                media.displayTitle(LocalTitleLanguage.current),
                style = MaterialTheme.typography.titleLarge,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            media.synopsis?.takeIf { it.isNotBlank() }?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button(onClick = { onWatch(media) }) {
                    Icon(Icons.Filled.PlayArrow, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.home_watch), modifier = Modifier.padding(start = 6.dp))
                }
                FilledTonalButton(onClick = { onMediaClick(media) }) {
                    Icon(Icons.Outlined.Info, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.home_details), modifier = Modifier.padding(start = 6.dp))
                }
            }
        }
    }
}

/** Localized status pill of a provider status (`RELEASING`, `FINISHED`, `NOT_YET_RELEASED`, `CANCELLED`, `HIATUS`). */
@Composable
private fun statusLabel(status: String?): Pair<String, PillTone>? = when (status?.uppercase()) {
    "RELEASING" -> stringResource(R.string.home_status_releasing) to PillTone.OK
    "FINISHED" -> stringResource(R.string.home_status_finished) to PillTone.NEUTRAL
    "NOT_YET_RELEASED" -> stringResource(R.string.home_status_upcoming) to PillTone.WARN
    "CANCELLED" -> stringResource(R.string.home_status_cancelled) to PillTone.ERR
    "HIATUS" -> stringResource(R.string.home_status_hiatus) to PillTone.WARN
    else -> null
}

@Composable
internal fun SectionHeader(title: String, onSeeAll: (() -> Unit)?) {
    Row(Modifier.fillMaxWidth().padding(end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        ListGroupHeader(title, Modifier.weight(1f))
        if (onSeeAll != null) {
            TextButton(onClick = onSeeAll) { Text(stringResource(R.string.home_see_all)) }
        }
    }
}

@Composable
private fun ContinueWatchingRow(entries: List<HistoryEntry>, onClick: (HistoryEntry) -> Unit, onSeeAll: () -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        SectionHeader(stringResource(R.string.home_continue_watching), onSeeAll)
        LazyRow(
            contentPadding = PaddingValues(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            items(entries, key = { it.episodeId }, contentType = { "continue" }) { entry ->
                ContinueCard(entry, onClick = { onClick(entry) })
            }
        }
    }
}

/** Prototype `.cont`: cover with progress, title, "Episodio N · Quedan M min", play affordance. */
@Composable
private fun ContinueCard(entry: HistoryEntry, onClick: () -> Unit) {
    val description = stringResource(R.string.home_continue_play, entry.title, entry.episode)
    Surface(
        onClick = onClick,
        shape = MaterialTheme.shapes.large,
        color = MaterialTheme.colorScheme.surfaceContainer,
        modifier = Modifier.width(CONTINUE_WIDTH).semantics { contentDescription = description },
    ) {
        Row(
            Modifier.padding(8.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            MediaCover(url = entry.poster, modifier = Modifier.width(56.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(entry.title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                val left = entry.durationSec?.let { d -> ((d - entry.progressSec).coerceAtLeast(0) + 59) / 60 }
                val sub = buildString {
                    append(stringResource(R.string.home_continue_episode, entry.episode))
                    if (left != null && !entry.completed) {
                        append(" · ")
                        append(stringResource(R.string.home_continue_left, left))
                    }
                }
                Text(
                    sub,
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                entry.progressFraction()?.takeIf { !entry.completed }?.let { p ->
                    LinearProgressIndicator(progress = { p }, modifier = Modifier.fillMaxWidth().padding(top = 4.dp))
                }
            }
            Surface(shape = CircleShape, color = MaterialTheme.colorScheme.primary, modifier = Modifier.size(36.dp)) {
                Box(contentAlignment = Alignment.Center) {
                    Icon(Icons.Filled.PlayArrow, contentDescription = null, tint = MaterialTheme.colorScheme.onPrimary)
                }
            }
        }
    }
}

@Composable
private fun GenreChips(genres: List<String>, onGenre: (String) -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        SectionHeader(stringResource(R.string.home_genres), onSeeAll = null)
        Row(
            Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            genres.forEach { genre ->
                AssistChip(onClick = { onGenre(genre) }, label = { Text(genre) })
            }
        }
    }
}

@Composable
private fun FeedRowSection(row: FeedRow, gate: MatureGate, onMediaClick: (Media) -> Unit, onRetry: () -> Unit, onSeeAll: () -> Unit) {
    Column(Modifier.fillMaxWidth()) {
        SectionHeader(stringResource(row.kind.labelRes()), onSeeAll)
        when (val s = row.state) {
            LoadState.Loading -> SkeletonRow()
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
                    GatedMediaCard(media = media, gate = gate, onClick = { onMediaClick(media) }, width = CARD_WIDTH)
                }
            }
        }
    }
}

@Composable
internal fun SkeletonRow() {
    if (!rememberAfterDelay()) {
        Box(Modifier.fillMaxWidth().posterHeight())
        return
    }
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

/** The lazy `custom/home` rows of extensions: skeleton while loading, inline error + retry, nothing when no extension has one. */
@Composable
private fun ExtensionRows(
    state: LoadState<List<ExtensionHomeRow>>?,
    names: Map<String, String>,
    onRetry: () -> Unit,
    onOpenProvider: (String, String) -> Unit,
    onItemClick: (String, String, String) -> Unit,
) {
    when (state) {
        null, LoadState.Loading -> Column(Modifier.fillMaxWidth()) {
            SectionHeader(stringResource(R.string.home_ext_rows), onSeeAll = null)
            SkeletonRow()
        }
        is LoadState.Err -> Column(Modifier.fillMaxWidth()) {
            SectionHeader(stringResource(R.string.home_ext_rows), onSeeAll = null)
            ErrorState(
                message = stringResource(R.string.home_ext_rows_error) + " " + state.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = onRetry,
                compact = true,
            )
        }
        is LoadState.Ok -> Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            state.value.forEach { row ->
                Column(Modifier.fillMaxWidth()) {
                    SectionHeader(row.title, onSeeAll = { onOpenProvider(row.extensionId, row.sourceId) })
                    Text(
                        stringResource(R.string.home_ext_row_from, HomeLogic.extensionRowSubtitle(row, names)),
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 16.dp).padding(bottom = 8.dp),
                    )
                    LazyRow(
                        contentPadding = PaddingValues(horizontal = 16.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                    ) {
                        items(row.items, key = { it.id }, contentType = { "ext-item" }) { item ->
                            MediaCard(
                                title = item.title,
                                coverUrl = item.poster,
                                onClick = { onItemClick(row.extensionId, row.sourceId, item.id) },
                                modifier = Modifier.width(CARD_WIDTH),
                            )
                        }
                    }
                }
            }
        }
    }
}

internal fun Modifier.posterHeight(): Modifier = this.then(Modifier.height(CARD_WIDTH / COVER_ASPECT_RATIO))

internal fun FeedKind.labelRes(): Int = when (this) {
    FeedKind.TRENDING -> R.string.home_feed_trending
    FeedKind.POPULAR -> R.string.home_feed_popular
    FeedKind.TOP_RATED -> R.string.home_feed_top_rated
    FeedKind.AIRING -> R.string.home_feed_airing
    FeedKind.UPCOMING -> R.string.home_feed_upcoming
    FeedKind.SEASONAL -> R.string.home_feed_seasonal
    FeedKind.RECENTLY_COMPLETED -> R.string.home_feed_completed
}

private val HERO_HEIGHT = 260.dp
internal val CARD_WIDTH = 120.dp
private val CONTINUE_WIDTH = 280.dp
private const val SKELETON_COUNT = 6
private const val HERO_AUTOPLAY_MS = 9_000L
