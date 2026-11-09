// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

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
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.AutoStories
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.SearchOff
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SuggestionChip
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
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
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.novel.NovelRepositoryImpl
import app.tatakai.next.ui.labels.readLabelRes
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.state.screenInsets
import coil3.compose.AsyncImage
import org.koin.compose.viewmodel.koinViewModel
import app.tatakai.next.ui.R as UiR

/**
 * `Route.Novels(query)` (checklist novel.route / novel.home / novel.search): hero, tendencias, populares, géneros, "Seguir leyendo",
 * actualizaciones de las que sigues, búsqueda con filtros (género, estado, origen, orden). [onMediaClick] opens `Route.NovelDetails`.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun NovelHomeScreen(
    onMediaClick: (Media) -> Unit,
    onBack: (() -> Unit)?,
    onOpenReader: (NovelReaderArgs) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
    initialQuery: String? = null,
) {
    val vm: NovelHomeViewModel = koinViewModel()
    LaunchedEffect(initialQuery) { vm.applyInitial(initialQuery) }
    val home by vm.home.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val filters by vm.filters.collectAsStateWithLifecycle()
    val results by vm.results.collectAsStateWithLifecycle()
    val lang = settings.mangaTitleLanguage

    Scaffold(
        modifier = modifier,
        contentWindowInsets = screenInsets,
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.novel_title)) },
                navigationIcon = { if (onBack != null) IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) } },
                actions = { IconButton(onClick = vm::refresh) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.novel_refresh)) } },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp)) {
                OutlinedTextField(
                    value = filters.query,
                    onValueChange = vm::setQuery,
                    singleLine = true,
                    leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                    trailingIcon = { if (filters.active) IconButton(onClick = vm::clear) { Icon(Icons.Outlined.Close, contentDescription = stringResource(R.string.novel_search_clear)) } },
                    placeholder = { Text(stringResource(R.string.novel_search_hint)) },
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                    keyboardActions = KeyboardActions(onSearch = { vm.search() }),
                    modifier = Modifier.fillMaxWidth(),
                )
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    NovelFilterDropdown(
                        stringResource(R.string.novel_filter_genre), filters.genre, listOf<String?>(null) + home.genres,
                        optionLabel = { it ?: stringResource(R.string.novel_filter_any) },
                    ) { v -> vm.setFilters { it.copy(genre = v) } }
                    NovelFilterDropdown(
                        stringResource(R.string.novel_filter_status), novelStatusLabel(filters.status), listOf<String?>(null) + NovelFilters.STATUSES,
                        optionLabel = { novelStatusLabel(it) ?: stringResource(R.string.novel_filter_any) },
                    ) { v -> vm.setFilters { it.copy(status = v) } }
                    NovelFilterDropdown(
                        stringResource(R.string.novel_filter_origin), novelOriginLabel(filters.origin), listOf<String?>(null) + NovelFilters.ORIGINS,
                        optionLabel = { novelOriginLabel(it) ?: stringResource(R.string.novel_filter_any) },
                    ) { v -> vm.setFilters { it.copy(origin = v) } }
                    NovelFilterDropdown(stringResource(R.string.novel_filter_sort), novelSortLabel(filters.sort), NovelFilters.SORTS, optionLabel = { novelSortLabel(it) }) { v ->
                        vm.setFilters { it.copy(sort = v) }
                    }
                }
            }
            if (filters.active) {
                when (val st = results.state) {
                    null, LoadState.Loading -> if (rememberAfterDelay()) LoadingState()
                    is LoadState.Err -> NovelError(st.cause, vm::search)
                    is LoadState.Ok -> if (st.value.isEmpty()) {
                        EmptyState(icon = Icons.Outlined.SearchOff, title = stringResource(R.string.novel_search_empty), message = stringResource(R.string.novel_search_empty_message))
                    } else {
                        val grid = rememberLazyGridState()
                        LaunchedEffect(grid, st.value.size) {
                            snapshotFlow { grid.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0 }.collect { if (it >= st.value.size - 6) vm.loadMore() }
                        }
                        LazyVerticalGrid(
                            columns = GridCells.Adaptive(NOVEL_CARD_WIDTH),
                            state = grid,
                            contentPadding = PaddingValues(16.dp),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                            verticalArrangement = Arrangement.spacedBy(16.dp),
                            modifier = Modifier.fillMaxSize(),
                        ) {
                            items(st.value.size, key = { st.value[it].ref.anilistId ?: it }) { i ->
                                val m = st.value[i]
                                NovelCard(m.displayTitle(lang), m.cover, listOfNotNull(m.year?.toString(), novelStatusLabel(m.status)).joinToString(" · ").ifBlank { null }, onClick = { onMediaClick(m) })
                            }
                            if (results.loadingMore) item(span = { GridItemSpan(maxLineSpan) }) { LinearProgressIndicator(Modifier.fillMaxWidth().padding(8.dp)) }
                        }
                    }
                }
            } else {
                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                    item(key = "hero") {
                        when (val hero = home.hero) {
                            is LoadState.Ok -> if (hero.value.isNotEmpty()) {
                                HeroCarousel(items = hero.value, key = { it.ref.anilistId ?: it.title.hashCode() }, modifier = Modifier.padding(vertical = 8.dp)) { m, _ ->
                                    Box(Modifier.fillMaxWidth().height(220.dp).clickable { onMediaClick(m) }) {
                                        AsyncImage(model = m.banner ?: m.cover, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                                        Box(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(Color.Transparent, MaterialTheme.colorScheme.scrim.copy(alpha = 0.85f)))))
                                        Column(Modifier.align(Alignment.BottomStart).padding(16.dp)) {
                                            Text(m.displayTitle(lang), style = MaterialTheme.typography.titleLarge, color = Color.White, maxLines = 2, overflow = TextOverflow.Ellipsis)
                                            Button(onClick = { onMediaClick(m) }, modifier = Modifier.padding(top = 8.dp)) { Text(stringResource(R.string.novel_hero_open)) }
                                        }
                                    }
                                }
                            }
                            LoadState.Loading -> if (rememberAfterDelay()) Box(Modifier.fillMaxWidth().height(200.dp)) { LoadingState() }
                            is LoadState.Err -> NovelError(hero.cause, vm::refresh)
                        }
                    }
                    if (home.lastRead.isNotEmpty()) {
                        item(key = "continue") {
                            NovelSectionHeader(stringResource(R.string.novel_continue_reading))
                            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                items(home.lastRead, key = { it.mediaId }) { p ->
                                    NovelCard(
                                        title = p.title,
                                        cover = p.poster,
                                        subtitle = stringResource(R.string.novel_continue_line, ChapterText.formatNumber(p.chapterNumber) ?: "?", (p.offset * 100).toInt()),
                                        progress = p.offset,
                                        onClick = {
                                            onOpenReader(
                                                NovelReaderArgs(
                                                    p.mediaId, p.title, p.poster, p.extensionId, p.sourceId, p.itemId, p.chapterId, p.chapterNumber, p.paragraph,
                                                    anilistId = NovelRepositoryImpl.refOf(p.mediaId)?.anilistId,
                                                ),
                                            )
                                        },
                                    )
                                }
                            }
                        }
                    }
                    if (home.followedUpdates.isNotEmpty()) {
                        item(key = "updates") {
                            NovelSectionHeader(stringResource(R.string.novel_followed_updates))
                            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                items(home.followedUpdates, key = { it.first.ref.anilistId ?: it.first.title.hashCode() }) { (entry, ch) ->
                                    val mediaId = NovelRepositoryImpl.mediaKey(entry.ref) ?: return@items
                                    NovelCard(
                                        title = entry.title,
                                        cover = entry.poster,
                                        subtitle = stringResource(R.string.novel_followed_new_chapter, ChapterText.formatNumber(ch.number) ?: "?"),
                                        onClick = {
                                            vm.dismissUpdate(mediaId)
                                            onOpenReader(NovelReaderArgs(mediaId, entry.title, entry.poster, ch.extensionId, ch.sourceId, ch.itemId, ch.id, ch.number, anilistId = entry.ref.anilistId))
                                        },
                                    )
                                }
                            }
                        }
                    }
                    item(key = "trending") { NovelRow(stringResource(R.string.novel_row_trending), home.trending, lang, vm::refresh, onMediaClick) }
                    item(key = "popular") { NovelRow(stringResource(R.string.novel_row_popular), home.popular, lang, vm::refresh, onMediaClick) }
                    if (home.genres.isNotEmpty()) {
                        item(key = "genres") {
                            NovelSectionHeader(stringResource(R.string.novel_row_genres))
                            FlowRow(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                home.genres.forEach { g -> SuggestionChip(onClick = { vm.setFilters { NovelFilters(genre = g) } }, label = { Text(g) }) }
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Library › Novelas (`LibraryTab.NOVELS`; checklist novel.readlist). Same shape as `MangaLibraryTab`. */
@Composable
fun NovelLibraryTab(onOpenDetails: (anilistId: Int) -> Unit, onOpenAccounts: () -> Unit, modifier: Modifier = Modifier) {
    val vm: NovelLibraryViewModel = koinViewModel()
    val entries by vm.entries.collectAsStateWithLifecycle()
    val lastRead by vm.lastRead.collectAsStateWithLifecycle()
    val linked by vm.linked.collectAsStateWithLifecycle()
    val syncing by vm.syncing.collectAsStateWithLifecycle()
    var filter by rememberSaveable { mutableStateOf<ListStatus?>(ListStatus.WATCHING) }
    LaunchedEffect(linked) { if (linked) vm.sync() }
    Column(modifier) {
        if (syncing) LinearProgressIndicator(Modifier.fillMaxWidth())
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FilterChip(selected = filter == null, onClick = { filter = null }, label = { Text(stringResource(R.string.novel_library_all)) })
                NOVEL_READ_STATUSES.forEach { s ->
                    FilterChip(selected = filter == s, onClick = { filter = s }, label = { Text("${stringResource(s.readLabelRes())} · ${entries?.count { it.status == s } ?: 0}") })
                }
            }
            if (linked) IconButton(onClick = vm::sync, enabled = !syncing) { Icon(Icons.Outlined.Sync, contentDescription = stringResource(R.string.novel_library_sync)) }
        }
        val list = entries ?: return@Column
        if (list.isEmpty() && lastRead.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.AutoStories,
                title = stringResource(R.string.novel_library_empty),
                message = stringResource(if (linked) R.string.novel_library_empty_linked else R.string.novel_library_empty_message),
                actionLabel = if (linked) null else stringResource(R.string.novel_library_link),
                onAction = if (linked) null else onOpenAccounts,
                modifier = Modifier.fillMaxSize(),
            )
            return@Column
        }
        val shown = if (filter == null) list else list.filter { it.status == filter }
        LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
            if (lastRead.isNotEmpty()) {
                item(key = "continue") {
                    NovelSectionHeader(stringResource(R.string.novel_continue_reading))
                    LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        items(lastRead, key = { it.mediaId }) { p ->
                            val id = NovelRepositoryImpl.refOf(p.mediaId)?.anilistId
                            NovelCard(p.title, p.poster, stringResource(R.string.novel_continue_line, ChapterText.formatNumber(p.chapterNumber) ?: "?", (p.offset * 100).toInt()), onClick = { id?.let(onOpenDetails) }, progress = p.offset)
                        }
                    }
                }
            }
            if (shown.isEmpty()) item(key = "none") { Text(stringResource(R.string.novel_library_none_in_status), modifier = Modifier.padding(16.dp), color = MaterialTheme.colorScheme.onSurfaceVariant) }
            items(shown, key = { "${it.ref.anilistId}-${it.ref.malId}" }) { e ->
                ListItem(
                    leadingContent = { Box(Modifier.width(48.dp)) { MediaCover(url = e.poster, modifier = Modifier.fillMaxWidth()) } },
                    headlineContent = { Text(e.title, maxLines = 2, overflow = TextOverflow.Ellipsis) },
                    supportingContent = { Text(stringResource(e.status.readLabelRes()) + " · " + stringResource(R.string.novel_progress_label, e.progress, e.totalChapters?.toString() ?: "?")) },
                    modifier = Modifier.clickable(enabled = e.ref.anilistId != null) { e.ref.anilistId?.let(onOpenDetails) },
                )
            }
        }
    }
}

internal val NOVEL_READ_STATUSES = listOf(ListStatus.WATCHING, ListStatus.PLANNING, ListStatus.COMPLETED, ListStatus.PAUSED, ListStatus.DROPPED, ListStatus.REPEATING)
