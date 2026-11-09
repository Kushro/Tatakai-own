// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material.icons.outlined.Shield
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.HeroCarousel
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.manga.ChapterText
import app.tatakai.next.manga.MangaRepositoryImpl
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.Media
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.state.screenInsets
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import org.koin.compose.viewmodel.koinViewModel
import java.time.LocalDate
import app.tatakai.next.ui.R as UiR

/**
 * `Route.Manga(query)` (checklist manga.route / manga.home / manga.home.followed-updates / manga.search / manga.adult-gate): hero,
 * Tendencias, Populares, Mejor valorados, géneros, "Seguir leyendo" (`MangaRepository.lastRead`), novedades de lo que lees, zona 18+ con
 * puerta (`AppSettings.showAdult` + `matureContent`), búsqueda con filtros (origen JP/KR/CN, estado, género, año, orden). [initialQuery]
 * comes from the Search type chip. [onMediaClick] opens `Route.Details(MANGA, id)`. [onBack] null on wide windows (rail destination).
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MangaHomeScreen(
    onMediaClick: (Media) -> Unit,
    onBack: (() -> Unit)?,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
    initialQuery: String? = null,
) {
    val vm: MangaHomeViewModel = koinViewModel()
    LaunchedEffect(initialQuery) { vm.applyInitial(initialQuery) }
    val home by vm.home.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val prefs by vm.prefs.collectAsStateWithLifecycle()
    val filters by vm.filters.collectAsStateWithLifecycle()
    val results by vm.results.collectAsStateWithLifecycle()
    val lang = settings.mangaTitleLanguage
    var pendingMature by remember { mutableStateOf<Media?>(null) }
    var gateOpen by rememberSaveable { mutableStateOf(false) }

    // Adult titles of the 18+ zone follow matureContent: WARN asks once per title, BLUR blurs the poster, SHOW opens directly.
    val openAdult: (Media) -> Unit = { media ->
        val id = media.ref.anilistId
        if (settings.matureContent == MatureContentMode.WARN && id != null && id !in prefs.matureAccepted) pendingMature = media else onMediaClick(media)
    }

    Scaffold(
        modifier = modifier,
        contentWindowInsets = screenInsets,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.manga_title)) },
                navigationIcon = {
                    if (onBack != null) {
                        IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) }
                    }
                },
                actions = {
                    IconButton(onClick = vm::refresh) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.manga_refresh)) }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            SearchHeader(
                filters = filters,
                genres = home.genres,
                onQuery = vm::setQuery,
                onSubmit = vm::search,
                onFilters = vm::setFilters,
                onClear = vm::clearSearch,
            )
            if (filters.active) {
                SearchResultsGrid(results, lang, onRetry = vm::search, onLoadMore = vm::loadMore, onClick = onMediaClick)
            } else {
                HomeContent(
                    home = home,
                    settings = settings,
                    adultAccepted = prefs.adultGateAccepted,
                    onRefresh = vm::refresh,
                    onMediaClick = onMediaClick,
                    onAdultClick = openAdult,
                    onOpenReader = onOpenReader,
                    onGenre = { g -> vm.setFilters { MangaFilters(genre = g) } },
                    onShowAdult = { gateOpen = true },
                    onHideAdult = vm::hideAdultZone,
                    onRetryAdult = vm::loadAdultZone,
                    onDismissUpdate = vm::dismissUpdate,
                    onOpenAccounts = onOpenAccounts,
                )
            }
        }
    }

    if (gateOpen) {
        AlertDialog(
            onDismissRequest = { gateOpen = false },
            icon = { Icon(Icons.Outlined.Shield, contentDescription = null) },
            title = { Text(stringResource(R.string.manga_adult_gate_title)) },
            text = { Text(stringResource(R.string.manga_adult_gate_message)) },
            confirmButton = {
                TextButton(onClick = {
                    gateOpen = false
                    vm.acceptAdultGate()
                }) { Text(stringResource(R.string.manga_adult_gate_confirm)) }
            },
            dismissButton = { TextButton(onClick = { gateOpen = false }) { Text(stringResource(UiR.string.ui_cancel)) } },
        )
    }
    pendingMature?.let { media ->
        MatureWarningDialog(
            onConfirm = {
                pendingMature = null
                media.ref.anilistId?.let(vm::acceptMature)
                onMediaClick(media)
            },
            onDismiss = { pendingMature = null },
        )
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SearchHeader(
    filters: MangaFilters,
    genres: List<String>,
    onQuery: (String) -> Unit,
    onSubmit: () -> Unit,
    onFilters: ((MangaFilters) -> MangaFilters) -> Unit,
    onClear: () -> Unit,
) {
    Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
        OutlinedTextField(
            value = filters.query,
            onValueChange = onQuery,
            singleLine = true,
            leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
            trailingIcon = {
                if (filters.active) IconButton(onClick = onClear) { Icon(Icons.Outlined.Close, contentDescription = stringResource(R.string.manga_search_clear)) }
            },
            placeholder = { Text(stringResource(R.string.manga_search_hint)) },
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
            keyboardActions = KeyboardActions(onSearch = { onSubmit() }),
            modifier = Modifier.fillMaxWidth(),
        )
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FilterDropdown(
                label = stringResource(R.string.manga_filter_origin),
                value = originLabel(filters.origin),
                options = listOf<String?>(null) + MangaFilters.ORIGINS,
                optionLabel = { originLabel(it) ?: stringResource(R.string.manga_filter_any) },
                onSelect = { v -> onFilters { it.copy(origin = v) } },
            )
            FilterDropdown(
                label = stringResource(R.string.manga_filter_status),
                value = anilistStatusLabel(filters.status),
                options = listOf<String?>(null) + MangaFilters.STATUSES,
                optionLabel = { anilistStatusLabel(it) ?: stringResource(R.string.manga_filter_any) },
                onSelect = { v -> onFilters { it.copy(status = v) } },
            )
            FilterDropdown(
                label = stringResource(R.string.manga_filter_genre),
                value = filters.genre,
                options = listOf<String?>(null) + genres,
                optionLabel = { it ?: stringResource(R.string.manga_filter_any) },
                onSelect = { v -> onFilters { it.copy(genre = v) } },
            )
            val thisYear = remember { LocalDate.now().year }
            FilterDropdown(
                label = stringResource(R.string.manga_filter_year),
                value = filters.year?.toString(),
                options = listOf<Int?>(null) + (thisYear + 1 downTo 1950).toList(),
                optionLabel = { it?.toString() ?: stringResource(R.string.manga_filter_any) },
                onSelect = { v -> onFilters { it.copy(year = v) } },
            )
            FilterDropdown(
                label = stringResource(R.string.manga_filter_sort),
                value = sortLabel(filters.sort),
                options = MangaFilters.SORTS,
                optionLabel = { sortLabel(it) },
                onSelect = { v -> onFilters { it.copy(sort = v) } },
            )
        }
    }
}

@Composable
internal fun <T> FilterDropdown(label: String, value: String?, options: List<T>, optionLabel: @Composable (T) -> String, onSelect: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        FilterChip(
            selected = value != null,
            onClick = { open = true },
            label = { Text(if (value != null) "$label: $value" else label) },
        )
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { option ->
                DropdownMenuItem(text = { Text(optionLabel(option)) }, onClick = {
                    open = false
                    onSelect(option)
                })
            }
        }
    }
}

@Composable
private fun SearchResultsGrid(
    results: SearchResultsUi,
    lang: app.tatakai.next.model.TitleLanguage,
    onRetry: () -> Unit,
    onLoadMore: () -> Unit,
    onClick: (Media) -> Unit,
) {
    when (val state = results.state) {
        null, LoadState.Loading -> if (rememberAfterDelay()) LoadingState()
        is LoadState.Err -> ErrorBlock(state.cause, onRetry)
        is LoadState.Ok -> if (state.value.isEmpty()) {
            EmptyState(icon = Icons.Outlined.SearchOff, title = stringResource(R.string.manga_search_empty), message = stringResource(R.string.manga_search_empty_message))
        } else {
            val gridState = androidx.compose.foundation.lazy.grid.rememberLazyGridState()
            LaunchedEffect(gridState, state.value.size) {
                snapshotFlow { gridState.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }.collect { last ->
                    if (last >= state.value.size - 6) onLoadMore()
                }
            }
            LazyVerticalGrid(
                columns = GridCells.Adaptive(CARD_WIDTH),
                state = gridState,
                contentPadding = PaddingValues(16.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                modifier = Modifier.fillMaxSize(),
            ) {
                items(state.value, key = { it.ref.anilistId ?: it.title.hashCode() }) { media ->
                    MangaCard(media, lang, onClick = { onClick(media) }, subtitle = listOfNotNull(media.year?.toString(), anilistStatusLabel(media.status)).joinToString(" · ").ifBlank { null })
                }
                if (results.loadingMore) {
                    item(span = { GridItemSpan(maxLineSpan) }) { LinearProgressIndicator(Modifier.fillMaxWidth().padding(8.dp)) }
                }
                results.moreError?.let { err ->
                    item(span = { GridItemSpan(maxLineSpan) }) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(err.userMessage(), modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodySmall)
                            TextButton(onClick = onLoadMore) { Text(stringResource(UiR.string.ui_retry)) }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun HomeContent(
    home: app.tatakai.next.model.MangaHome,
    settings: AppSettings,
    adultAccepted: Boolean,
    onRefresh: () -> Unit,
    onMediaClick: (Media) -> Unit,
    onAdultClick: (Media) -> Unit,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onGenre: (String) -> Unit,
    onShowAdult: () -> Unit,
    onHideAdult: () -> Unit,
    onRetryAdult: () -> Unit,
    onDismissUpdate: (String) -> Unit,
    onOpenAccounts: () -> Unit,
) {
    val lang = settings.mangaTitleLanguage
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
        // Hero
        item(key = "hero") {
            when (val hero = home.hero) {
                is LoadState.Ok -> if (hero.value.isNotEmpty()) {
                    HeroCarousel(items = hero.value, key = { it.ref.anilistId ?: it.title.hashCode() }, modifier = Modifier.padding(vertical = 8.dp)) { media, _ ->
                        HeroSlide(media, media.displayTitle(lang), onClick = { onMediaClick(media) })
                    }
                }
                LoadState.Loading -> if (rememberAfterDelay()) Box(Modifier.fillMaxWidth().height(200.dp)) { LoadingState() }
                is LoadState.Err -> ErrorBlock(hero.cause, onRefresh)
            }
        }
        // Continue reading
        if (home.lastRead.isNotEmpty()) {
            item(key = "continue") {
                SectionHeader(stringResource(R.string.manga_continue_reading))
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    items(home.lastRead, key = { it.mediaId }) { p ->
                        ContinueCard(
                            title = p.title,
                            cover = p.poster,
                            line = stringResource(R.string.manga_continue_line, ChapterText.formatNumber(p.chapterNumber) ?: "?", p.page + 1, p.totalPages.coerceAtLeast(1)),
                            progress = if (p.totalPages > 0) (p.page + 1f) / p.totalPages else 0f,
                            onClick = {
                                onOpenReader(
                                    MangaReaderArgs(
                                        mediaId = p.mediaId, title = p.title, poster = p.poster, extensionId = p.extensionId, sourceId = p.sourceId,
                                        itemId = p.itemId, chapterId = p.chapterId, chapterNumber = p.chapterNumber, startPage = p.page,
                                        anilistId = MangaRepositoryImpl.refOf(p.mediaId)?.anilistId,
                                    ),
                                )
                            },
                        )
                    }
                }
            }
        }
        // New chapters of what you read
        if (home.followedUpdates.isNotEmpty()) {
            item(key = "updates") {
                SectionHeader(stringResource(R.string.manga_followed_updates))
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    items(home.followedUpdates, key = { it.first.ref.anilistId ?: it.first.title.hashCode() }) { (entry, chapter) ->
                        val mediaId = MangaRepositoryImpl.mediaKey(entry.ref) ?: return@items
                        ContinueCard(
                            title = entry.title,
                            cover = entry.poster,
                            line = stringResource(R.string.manga_followed_new_chapter, ChapterText.formatNumber(chapter.number) ?: "?"),
                            progress = null,
                            onClick = {
                                onDismissUpdate(mediaId)
                                onOpenReader(
                                    MangaReaderArgs(
                                        mediaId = mediaId, title = entry.title, poster = entry.poster, extensionId = chapter.extensionId,
                                        sourceId = chapter.sourceId, itemId = chapter.itemId, chapterId = chapter.id, chapterNumber = chapter.number,
                                        anilistId = entry.ref.anilistId,
                                    ),
                                )
                            },
                        )
                    }
                }
            }
        }
        item(key = "trending") { MediaRowSection(stringResource(R.string.manga_row_trending), home.trending, lang, onRefresh, onMediaClick) }
        item(key = "popular") { MediaRowSection(stringResource(R.string.manga_row_popular), home.popular, lang, onRefresh, onMediaClick) }
        item(key = "top") { MediaRowSection(stringResource(R.string.manga_row_top_rated), home.topRated, lang, onRefresh, onMediaClick) }
        if (home.genres.isNotEmpty()) {
            item(key = "genres") {
                SectionHeader(stringResource(R.string.manga_row_genres))
                FlowRow(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    home.genres.forEach { g -> SuggestionChip(onClick = { onGenre(g) }, label = { Text(g) }) }
                }
            }
        }
        // 18+ zone (checklist manga.adult-gate): needs showAdult and the one-time gate; posters follow matureContent.
        item(key = "adult") {
            SectionHeader(stringResource(R.string.manga_adult_zone), action = if (home.adult != null) ({ TextButton(onClick = onHideAdult) { Text(stringResource(R.string.manga_adult_hide)) } }) else null)
            val adult = home.adult
            LaunchedEffect(settings.showAdult, adultAccepted, adult == null) {
                if (settings.showAdult && adultAccepted && adult == null) onRetryAdult()
            }
            when {
                !settings.showAdult -> Text(
                    stringResource(R.string.manga_adult_disabled),
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(horizontal = 16.dp),
                )
                !adultAccepted -> FilledTonalButton(onClick = onShowAdult, modifier = Modifier.padding(horizontal = 16.dp)) {
                    Icon(Icons.Outlined.Shield, contentDescription = null)
                    Text(stringResource(R.string.manga_adult_show), modifier = Modifier.padding(start = 8.dp))
                }
                adult == null -> if (rememberAfterDelay()) SkeletonRow()
                else -> MediaRowSectionBody(adult, lang, onRetryAdult, onAdultClick, blurred = settings.matureContent.blurs())
            }
        }
    }
}

@Composable
private fun MediaRowSectionBody(state: LoadState<List<Media>>, lang: app.tatakai.next.model.TitleLanguage, onRetry: () -> Unit, onClick: (Media) -> Unit, blurred: Boolean) {
    when (state) {
        LoadState.Loading -> if (rememberAfterDelay()) SkeletonRow()
        is LoadState.Err -> ErrorBlock(state.cause, onRetry)
        is LoadState.Ok -> if (state.value.isEmpty()) {
            Text(stringResource(R.string.manga_search_empty), modifier = Modifier.padding(horizontal = 16.dp))
        } else {
            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                items(state.value, key = { it.ref.anilistId ?: it.title.hashCode() }) { media ->
                    MangaCard(media, lang, onClick = { onClick(media) }, blurred = blurred)
                }
            }
        }
    }
}

@Composable
private fun HeroSlide(media: Media, title: String, onClick: () -> Unit) {
    Box(Modifier.fillMaxWidth().height(220.dp).clickable(onClick = onClick)) {
        AsyncImage(model = media.banner ?: media.cover, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        Box(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(Color.Transparent, MaterialTheme.colorScheme.scrim.copy(alpha = 0.85f)))))
        Column(Modifier.align(Alignment.BottomStart).padding(16.dp)) {
            Text(title, style = MaterialTheme.typography.titleLarge, color = Color.White, maxLines = 2, overflow = TextOverflow.Ellipsis)
            media.genres.take(3).takeIf { it.isNotEmpty() }?.let {
                Text(it.joinToString(" · "), style = MaterialTheme.typography.labelMedium, color = Color.White.copy(alpha = 0.85f))
            }
            Button(onClick = onClick, modifier = Modifier.padding(top = 8.dp)) { Text(stringResource(R.string.manga_hero_open)) }
        }
    }
}

@Composable
internal fun ContinueCard(title: String, cover: String?, line: String, progress: Float?, onClick: () -> Unit) {
    Column(Modifier.width(CARD_WIDTH).clickable(onClick = onClick)) {
        Box(contentAlignment = Alignment.BottomCenter) {
            app.tatakai.next.ui.media.MediaCover(url = cover, modifier = Modifier.fillMaxWidth())
            if (progress != null) LinearProgressIndicator(progress = { progress.coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth().padding(8.dp))
        }
        Text(title, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 6.dp))
        Text(line, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary, maxLines = 1)
    }
}
