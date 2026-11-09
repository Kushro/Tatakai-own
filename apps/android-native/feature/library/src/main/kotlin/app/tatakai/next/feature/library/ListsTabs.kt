// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalFoundationApi::class, ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.library

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.GridItemSpan
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.Sort
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.outlined.Add
import androidx.compose.material.icons.outlined.BookmarkAdd
import androidx.compose.material.icons.outlined.Bookmarks
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Delete
import androidx.compose.material.icons.outlined.Edit
import androidx.compose.material.icons.outlined.FavoriteBorder
import androidx.compose.material.icons.outlined.Remove
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.CharacterRef
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.ui.labels.labelRes
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.state.rememberIsOnline
import app.tatakai.next.ui.state.userMessage
import kotlinx.coroutines.launch
import org.koin.compose.koinInject
import org.koin.compose.viewmodel.koinViewModel
import kotlin.math.roundToInt
import app.tatakai.next.ui.R as UiR

// Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.3). Owner: worker q. Signatures FINAL (Details mounts
// ListButton; LibraryScreen mounts the tabs). Lists live in Room first (lists.local-only) and sync with AniList / MAL when linked.

/**
 * Library › Listas: status segmented (every [ListStatus], 15 §0.6) + sort / filter + grid of `ListEntry` cards (poster, title,
 * "progress / total", score); tap -> Details, long press -> editor sheet (status, progress, score, favorite, remove). Without a linked
 * account the list still works locally and the empty state offers [onOpenAccounts].
 */
@Composable
fun ListsTab(
    onOpenDetails: (anilistId: Int) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: ListsViewModel = koinViewModel()
    val state by vm.state.collectAsStateWithLifecycle()
    ListsEvents(vm)
    var editing by remember { mutableStateOf<ListEntry?>(null) }

    LazyVerticalGrid(
        columns = GridCells.Adaptive(minSize = 110.dp),
        modifier = modifier,
        contentPadding = PaddingValues(16.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        item(key = "controls", span = { GridItemSpan(maxLineSpan) }, contentType = "controls") {
            ListsControls(state, vm, onOpenAccounts)
        }
        when (val entries = state.entries) {
            LoadState.Loading -> item(key = "loading", span = { GridItemSpan(maxLineSpan) }) { DelayedListLoading() }
            is LoadState.Err -> item(key = "error", span = { GridItemSpan(maxLineSpan) }) {
                ErrorState(
                    message = entries.cause.userMessage(),
                    retryLabel = stringResource(UiR.string.ui_retry),
                    onRetry = vm::retry,
                    title = stringResource(R.string.library_lists_error),
                )
            }
            is LoadState.Ok -> if (entries.value.isEmpty()) {
                item(key = "empty", span = { GridItemSpan(maxLineSpan) }) {
                    val filtered = state.query.isNotBlank()
                    EmptyState(
                        icon = Icons.Outlined.Bookmarks,
                        title = stringResource(
                            if (filtered) R.string.library_lists_no_match else R.string.library_lists_empty_title,
                            stringResource(state.status.labelRes()),
                        ),
                        message = stringResource(
                            if (state.accounts.isEmpty()) R.string.library_lists_empty_local else R.string.library_lists_empty_linked,
                        ),
                        actionLabel = if (state.accounts.isEmpty()) stringResource(R.string.library_lists_link) else null,
                        onAction = if (state.accounts.isEmpty()) onOpenAccounts else null,
                    )
                }
            } else {
                items(entries.value, key = { "l-" + (it.ref.anilistId ?: 0) + "-" + (it.ref.malId ?: 0) }, contentType = { "entry" }) { entry ->
                    ListEntryCard(
                        entry = entry,
                        onClick = { entry.ref.anilistId?.let(onOpenDetails) ?: run { editing = entry } },
                        onLongClick = { editing = entry },
                    )
                }
            }
        }
    }
    editing?.let { entry ->
        ListEntryEditor(
            entry = entry,
            onDismiss = { editing = null },
            onStatus = { vm.setStatus(entry, it) },
            onProgress = { vm.setProgress(entry.ref, it) },
            onScore = { vm.setScore(entry.ref, it) },
            onFavorite = { vm.setFavorite(entry, it) },
        )
    }
}

@Composable
private fun ListsEvents(vm: ListsViewModel) {
    val snackbar = LocalAppSnackbar.current
    val resources = LocalContext.current.resources
    LaunchedEffect(vm) {
        vm.events.collect { event ->
            when (event) {
                ListsEvent.Queued -> snackbar.show(resources.getString(UiR.string.list_queued_offline))
                is ListsEvent.Failed -> snackbar.show(resources.getString(R.string.library_lists_write_failed))
            }
        }
    }
}

@Composable
private fun ListsControls(state: ListsUiState, vm: ListsViewModel, onOpenAccounts: () -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Segmented(
            options = ListStatus.entries.map { s ->
                val count = state.counts[s] ?: 0
                val label = stringResource(s.labelRes())
                SegmentedOption(s, if (count > 0) "$label · $count" else label)
            },
            selected = state.status,
            onSelect = vm::setStatusFilter,
        )
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedTextField(
                value = state.query,
                onValueChange = vm::setQuery,
                singleLine = true,
                leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                trailingIcon = if (state.query.isNotEmpty()) {
                    {
                        IconButton(onClick = { vm.setQuery("") }) {
                            Icon(Icons.Outlined.Close, contentDescription = stringResource(R.string.library_lists_clear_filter))
                        }
                    }
                } else {
                    null
                },
                placeholder = { Text(stringResource(R.string.library_lists_filter)) },
                modifier = Modifier.weight(1f),
            )
            SortMenu(state.sort, vm::setSort)
        }
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (state.accounts.isEmpty()) {
                Pill(stringResource(R.string.library_lists_local), tone = PillTone.NEUTRAL)
                TextButton(onClick = onOpenAccounts) { Text(stringResource(R.string.library_lists_link)) }
            } else {
                val anilist = stringResource(app.tatakai.next.model.ListProvider.ANILIST.labelRes())
                val mal = stringResource(app.tatakai.next.model.ListProvider.MAL.labelRes())
                Pill(
                    state.accounts.joinToString(" · ") { if (it.provider == app.tatakai.next.model.ListProvider.ANILIST) anilist else mal },
                    tone = PillTone.PRIMARY,
                )
                if (state.pendingChanges > 0) {
                    Pill(stringResource(R.string.library_lists_pending, state.pendingChanges), tone = PillTone.WARN)
                }
            }
        }
    }
}

@Composable
private fun SortMenu(sort: ListSort, onSort: (ListSort) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        val label = stringResource(R.string.library_lists_sort, stringResource(sort.labelRes()))
        IconButton(onClick = { open = true }, modifier = Modifier.semantics { contentDescription = label }) {
            Icon(Icons.AutoMirrored.Outlined.Sort, contentDescription = null)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            ListSort.entries.forEach { s ->
                DropdownMenuItem(
                    text = { Text(stringResource(s.labelRes())) },
                    onClick = {
                        open = false
                        onSort(s)
                    },
                    trailingIcon = if (s == sort) {
                        { Icon(Icons.Filled.Check, contentDescription = null) }
                    } else {
                        null
                    },
                )
            }
        }
    }
}

private fun ListSort.labelRes(): Int = when (this) {
    ListSort.UPDATED -> R.string.library_sort_updated
    ListSort.TITLE -> R.string.library_sort_title
    ListSort.PROGRESS -> R.string.library_sort_progress
    ListSort.SCORE -> R.string.library_sort_score
}

/** Poster card with "progress / total" and the score; long press opens the editor. */
@Composable
private fun ListEntryCard(entry: ListEntry, onClick: () -> Unit, onLongClick: () -> Unit, favoriteBadge: Boolean = false) {
    val editLabel = stringResource(R.string.library_lists_edit)
    Column(
        Modifier
            .clip(MaterialTheme.shapes.medium)
            .combinedClickable(onClick = onClick, onLongClick = onLongClick, onLongClickLabel = editLabel)
            .padding(bottom = 4.dp),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Box {
            MediaCover(url = entry.poster, modifier = Modifier.fillMaxWidth(), contentDescription = null)
            if (favoriteBadge || entry.favorite) {
                Icon(
                    Icons.Filled.Favorite,
                    contentDescription = stringResource(UiR.string.list_favorite),
                    tint = MaterialTheme.colorScheme.primary,
                    modifier = Modifier.align(Alignment.TopEnd).padding(6.dp).size(18.dp),
                )
            }
        }
        entry.progressFraction()?.let { LinearProgressIndicator(progress = { it }, modifier = Modifier.fillMaxWidth()) }
        Text(entry.title, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
        Text(
            listOfNotNull(
                entry.totalEpisodes?.let { stringResource(R.string.library_lists_progress_of, entry.progress, it) }
                    ?: stringResource(R.string.library_lists_progress, entry.progress),
                entry.score?.takeIf { it > 0 }?.let { stringResource(R.string.library_lists_score, it) },
            ).joinToString(" · "),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** Library › Favoritos: favorite titles as a poster grid + favorite characters; remove from the overflow / editor. */
@Composable
fun FavoritesTab(
    onOpenDetails: (anilistId: Int) -> Unit,
    onOpenAccounts: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: ListsViewModel = koinViewModel()
    val state by vm.state.collectAsStateWithLifecycle()
    ListsEvents(vm)
    var editing by remember { mutableStateOf<ListEntry?>(null) }

    when (val favorites = state.favorites) {
        LoadState.Loading -> Box(modifier) { DelayedListLoading() }
        is LoadState.Err -> ErrorState(
            message = favorites.cause.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = vm::retry,
            title = stringResource(R.string.library_favorites_error),
            modifier = modifier,
        )
        is LoadState.Ok -> if (favorites.value.isEmpty() && state.favoriteCharacters.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.FavoriteBorder,
                title = stringResource(R.string.library_favorites_empty_title),
                message = stringResource(
                    if (state.accounts.isEmpty()) R.string.library_favorites_empty_local else R.string.library_favorites_empty_linked,
                ),
                actionLabel = if (state.accounts.isEmpty()) stringResource(R.string.library_lists_link) else null,
                onAction = if (state.accounts.isEmpty()) onOpenAccounts else null,
                modifier = modifier,
            )
        } else {
            LazyVerticalGrid(
                columns = GridCells.Adaptive(minSize = 110.dp),
                modifier = modifier,
                contentPadding = PaddingValues(16.dp),
                horizontalArrangement = Arrangement.spacedBy(12.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                if (state.favoriteCharacters.isNotEmpty()) {
                    item(key = "chars", span = { GridItemSpan(maxLineSpan) }, contentType = "chars") {
                        FavoriteCharactersRow(state.favoriteCharacters, onRemove = vm::removeCharacter)
                    }
                }
                if (favorites.value.isNotEmpty()) {
                    item(key = "titles-header", span = { GridItemSpan(maxLineSpan) }) {
                        Text(stringResource(R.string.library_favorites_titles), style = MaterialTheme.typography.titleMedium)
                    }
                }
                items(favorites.value, key = { "f-" + (it.ref.anilistId ?: 0) + "-" + (it.ref.malId ?: 0) }, contentType = { "entry" }) { entry ->
                    ListEntryCard(
                        entry = entry,
                        onClick = { entry.ref.anilistId?.let(onOpenDetails) ?: run { editing = entry } },
                        onLongClick = { editing = entry },
                        favoriteBadge = true,
                    )
                }
            }
        }
    }
    editing?.let { entry ->
        ListEntryEditor(
            entry = entry,
            onDismiss = { editing = null },
            onStatus = { vm.setStatus(entry, it) },
            onProgress = { vm.setProgress(entry.ref, it) },
            onScore = { vm.setScore(entry.ref, it) },
            onFavorite = { vm.setFavorite(entry, it) },
        )
    }
}

@Composable
private fun FavoriteCharactersRow(characters: List<CharacterRef>, onRemove: (CharacterRef) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.library_favorites_characters), style = MaterialTheme.typography.titleMedium)
        LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            items(characters, key = { "c-" + it.id }) { c ->
                Column(Modifier.width(84.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Box {
                        MediaCover(url = c.image, modifier = Modifier.width(72.dp), shape = CircleShape, aspectRatio = 1f)
                        val removeLabel = stringResource(R.string.library_favorites_remove_named, c.name)
                        IconButton(
                            onClick = { onRemove(c) },
                            modifier = Modifier.align(Alignment.TopEnd).size(28.dp).semantics { contentDescription = removeLabel },
                        ) {
                            Icon(Icons.Filled.Favorite, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                        }
                    }
                    Text(c.name, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                }
            }
        }
        HorizontalDivider(color = MaterialTheme.colorScheme.outlineVariant)
    }
}

/**
 * Editor of one entry (classic `WatchlistButton` dialog): status chips, progress stepper, score slider (0..100, 0 = no score),
 * favorite and "Quitar de la lista". Each change saves at once (local first, queued for the linked providers).
 */
@Composable
internal fun ListEntryEditor(
    entry: ListEntry?,
    onDismiss: () -> Unit,
    onStatus: (ListStatus?) -> Unit,
    onProgress: (Int) -> Unit,
    onScore: (Int?) -> Unit,
    onFavorite: (Boolean) -> Unit,
    title: String = entry?.title.orEmpty(),
    totalEpisodes: Int? = entry?.totalEpisodes,
) {
    var status by remember(entry) { mutableStateOf(entry?.status) }
    var progress by remember(entry) { mutableIntStateOf(entry?.progress ?: 0) }
    var score by remember(entry) { mutableFloatStateOf((entry?.score ?: 0).toFloat()) }
    var favorite by remember(entry) { mutableStateOf(entry?.favorite == true) }
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text(title, style = MaterialTheme.typography.titleLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Text(stringResource(R.string.library_lists_status), style = MaterialTheme.typography.labelLarge)
            LazyRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                items(ListStatus.entries) { s ->
                    FilterChip(
                        selected = status == s,
                        onClick = {
                            status = s
                            onStatus(s)
                        },
                        label = { Text(stringResource(s.labelRes())) },
                    )
                }
            }
            if (status != null) {
                Text(stringResource(R.string.library_lists_progress_label), style = MaterialTheme.typography.labelLarge)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    FilledTonalIconButton(
                        onClick = {
                            progress = (progress - 1).coerceAtLeast(0)
                            onProgress(progress)
                        },
                        enabled = progress > 0,
                    ) { Icon(Icons.Outlined.Remove, contentDescription = stringResource(R.string.library_lists_progress_minus)) }
                    Text(
                        totalEpisodes?.let { stringResource(R.string.library_lists_progress_of, progress, it) }
                            ?: stringResource(R.string.library_lists_progress, progress),
                        style = MaterialTheme.typography.titleMedium,
                    )
                    FilledTonalIconButton(
                        onClick = {
                            progress = (progress + 1).let { p -> totalEpisodes?.let { p.coerceAtMost(it) } ?: p }
                            onProgress(progress)
                        },
                        enabled = totalEpisodes == null || progress < totalEpisodes,
                    ) { Icon(Icons.Outlined.Add, contentDescription = stringResource(R.string.library_lists_progress_plus)) }
                }
                Text(
                    if (score.roundToInt() > 0) {
                        stringResource(R.string.library_lists_score_label, score.roundToInt())
                    } else {
                        stringResource(R.string.library_lists_score_none)
                    },
                    style = MaterialTheme.typography.labelLarge,
                )
                Slider(
                    value = score,
                    onValueChange = { score = (it / 5f).roundToInt() * 5f },
                    onValueChangeFinished = { onScore(score.roundToInt().takeIf { it > 0 }) },
                    valueRange = 0f..100f,
                    steps = 19,
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton(onClick = {
                    favorite = !favorite
                    onFavorite(favorite)
                }) {
                    Icon(if (favorite) Icons.Filled.Favorite else Icons.Outlined.FavoriteBorder, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(
                        stringResource(if (favorite) R.string.library_favorites_remove else UiR.string.list_favorite),
                        modifier = Modifier.padding(start = 6.dp),
                    )
                }
                if (status != null) {
                    TextButton(onClick = {
                        status = null
                        onStatus(null)
                        onDismiss()
                    }) {
                        Icon(Icons.Outlined.Delete, contentDescription = null, modifier = Modifier.size(18.dp))
                        Text(stringResource(UiR.string.list_remove), modifier = Modifier.padding(start = 6.dp))
                    }
                }
            }
        }
    }
}

/**
 * "Añadir a lista" button of Details (classic `WatchlistButton`), mounted by worker o in the Details header / floating toolbar.
 * Label = current status or "Añadir a lista"; the menu lists every status, "Editar…" (progress / score sheet), "Quitar de la lista"
 * and the favorite toggle. Writes through `ListsRepository`; offline writes are queued (snackbar "Se sincronizará").
 * [compact] = icon-only variant for the floating toolbar.
 */
@Composable
fun ListButton(
    media: Media,
    modifier: Modifier = Modifier,
    compact: Boolean = false,
) {
    val lists = koinInject<ListsRepository>()
    val entryFlow = remember(media.ref) { lists.entry(media.ref) }
    val entry by entryFlow.collectAsState(initial = null)
    val accountsFlow = remember(lists) { lists.accounts }
    val accounts by accountsFlow.collectAsState()
    val online = rememberIsOnline()
    val scope = rememberCoroutineScope()
    val snackbar = LocalAppSnackbar.current
    val resources = LocalContext.current.resources
    var menu by remember { mutableStateOf(false) }
    var editor by rememberSaveable { mutableStateOf(false) }

    val write: (suspend () -> Result<Unit>) -> Unit = { block ->
        scope.launch {
            val result = try {
                block()
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                Result.failure(e)
            }
            if (result.isSuccess) {
                if (accounts.isNotEmpty() && !online) snackbar.show(resources.getString(UiR.string.list_queued_offline))
            } else {
                snackbar.show(resources.getString(R.string.library_lists_write_failed))
            }
        }
    }

    val current = entry
    val label = current?.let { stringResource(it.status.labelRes()) } ?: stringResource(UiR.string.list_add)
    Box(modifier) {
        if (compact) {
            FilledTonalIconButton(onClick = { menu = true }, modifier = Modifier.semantics { contentDescription = label }) {
                Icon(if (current != null) Icons.Outlined.Bookmarks else Icons.Outlined.BookmarkAdd, contentDescription = null)
            }
        } else {
            FilledTonalButton(onClick = { menu = true }) {
                Icon(if (current != null) Icons.Outlined.Bookmarks else Icons.Outlined.BookmarkAdd, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(label, modifier = Modifier.padding(horizontal = 6.dp))
                if (current?.favorite == true) {
                    Icon(Icons.Filled.Favorite, contentDescription = stringResource(UiR.string.list_favorite), modifier = Modifier.size(16.dp))
                }
                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = null, modifier = Modifier.size(18.dp))
            }
        }
        DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
            ListStatus.entries.forEach { s ->
                DropdownMenuItem(
                    text = { Text(stringResource(s.labelRes())) },
                    onClick = {
                        menu = false
                        write { lists.setStatus(media, s) }
                    },
                    trailingIcon = if (current?.status == s) {
                        { Icon(Icons.Filled.Check, contentDescription = null) }
                    } else {
                        null
                    },
                )
            }
            HorizontalDivider()
            if (current != null) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.library_lists_edit)) },
                    onClick = {
                        menu = false
                        editor = true
                    },
                    leadingIcon = { Icon(Icons.Outlined.Edit, contentDescription = null) },
                )
            }
            val fav = current?.favorite == true
            DropdownMenuItem(
                text = { Text(stringResource(if (fav) R.string.library_favorites_remove else UiR.string.list_favorite)) },
                onClick = {
                    menu = false
                    write { lists.setFavorite(media, !fav) }
                },
                leadingIcon = { Icon(if (fav) Icons.Filled.Favorite else Icons.Outlined.FavoriteBorder, contentDescription = null) },
            )
            if (current != null) {
                DropdownMenuItem(
                    text = { Text(stringResource(UiR.string.list_remove)) },
                    onClick = {
                        menu = false
                        write { lists.setStatus(media, null) }
                    },
                    leadingIcon = { Icon(Icons.Outlined.Delete, contentDescription = null) },
                )
            }
        }
    }
    if (editor && current != null) {
        ListEntryEditor(
            entry = current,
            onDismiss = { editor = false },
            onStatus = { s -> write { lists.setStatus(media, s) } },
            onProgress = { p -> write { lists.setProgress(media.ref, p) } },
            onScore = { sc -> write { lists.setScore(media.ref, sc) } },
            onFavorite = { f -> write { lists.setFavorite(media, f) } },
            title = media.title,
            totalEpisodes = current.totalEpisodes ?: media.episodes,
        )
    }
}

@Composable
private fun DelayedListLoading() {
    if (rememberAfterDelay()) Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) { LoadingState() }
}
