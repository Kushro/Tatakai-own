// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.extensions

import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.MenuBook
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.automirrored.outlined.Sort
import androidx.compose.material.icons.outlined.Bookmark
import androidx.compose.material.icons.outlined.BookmarkBorder
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.AppError
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ExtensionHomeItem
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.model.ProviderEntry
import app.tatakai.next.model.ProviderItem
import app.tatakai.next.model.SourcePrefs
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.state.userMessage
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf
import kotlin.math.roundToInt

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.4, worker t). Signatures FINAL.

/**
 * `Route.Provider(extensionId, sourceId)` (checklist ext.custom-providers.home / .list): the provider page of one custom source
 * (`ExtensionRepository.providerHome` rows + `providerSearch`); items → [onOpenItem]. The list of providers lives in ExtensionsScreen
 * (section "Proveedores" fed by `customSources()`), which navigates here with `onOpenProvider`.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ProviderScreen(
    extensionId: String,
    sourceId: String,
    onBack: () -> Unit,
    onOpenItem: (itemId: String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel: ProviderViewModel = koinViewModel(key = "provider-$extensionId/$sourceId", parameters = { parametersOf(extensionId, sourceId) })
    val state by viewModel.state.collectAsStateWithLifecycle()
    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = {
                    Column {
                        Text(state.source?.name ?: stringResource(R.string.extensions_provider_title), maxLines = 1, overflow = TextOverflow.Ellipsis)
                        state.source?.extensionName?.let {
                            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) }
                },
            )
        },
    ) { padding ->
        val listState = rememberLazyListState()
        val nearEnd by remember {
            derivedStateOf {
                val info = listState.layoutInfo
                val last = info.visibleItemsInfo.lastOrNull()?.index ?: 0
                info.totalItemsCount > 0 && last >= info.totalItemsCount - 3
            }
        }
        LaunchedEffect(nearEnd, state.results) { if (nearEnd && state.results is LoadState.Ok) viewModel.loadMore() }
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
            LazyColumn(
                state = listState,
                modifier = Modifier.fillMaxSize().widthIn(max = 960.dp),
                contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 32.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
            ) {
                item(key = "search", contentType = "search") {
                    OutlinedTextField(
                        value = state.query,
                        onValueChange = viewModel::setQuery,
                        leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
                        label = { Text(stringResource(R.string.extensions_provider_search)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
                val results = state.results
                if (results != null) {
                    when (results) {
                        LoadState.Loading -> item(key = "results-loading", contentType = "state") { Delayed() }
                        is LoadState.Err -> item(key = "results-error", contentType = "state") {
                            ErrorState(message = results.cause.userMessage(), retryLabel = stringResource(UiR.string.ui_retry), onRetry = viewModel::retrySearch)
                        }
                        is LoadState.Ok -> if (results.value.isEmpty()) {
                            item(key = "results-empty", contentType = "state") {
                                EmptyState(icon = Icons.Outlined.Search, title = stringResource(R.string.extensions_provider_no_results, state.query.trim()))
                            }
                        } else {
                            items(results.value, key = { "r:${it.id}" }, contentType = { "result" }) { item ->
                                ResultRow(item, onClick = { onOpenItem(item.id) })
                            }
                            if (state.loadingMore) item(key = "more", contentType = "state") { Delayed(Modifier.fillMaxWidth().height(64.dp)) }
                        }
                    }
                } else {
                    when (val home = state.home) {
                        LoadState.Loading -> item(key = "home-loading", contentType = "state") { Delayed() }
                        is LoadState.Err -> item(key = "home-error", contentType = "state") {
                            ErrorState(
                                message = home.cause.userMessage(),
                                retryLabel = stringResource(UiR.string.ui_retry),
                                onRetry = viewModel::load,
                                title = stringResource(R.string.extensions_provider_error),
                            )
                        }
                        is LoadState.Ok -> if (home.value.isEmpty()) {
                            item(key = "home-empty", contentType = "state") {
                                EmptyState(
                                    icon = Icons.Outlined.Hub,
                                    title = stringResource(R.string.extensions_provider_empty_title),
                                    message = stringResource(R.string.extensions_provider_empty),
                                )
                            }
                        } else {
                            items(home.value, key = { "row:${it.title}" }, contentType = { "row" }) { row ->
                                Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                    Text(row.title, style = MaterialTheme.typography.titleMedium, modifier = Modifier.semantics { heading() })
                                    LazyRow(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                                        items(row.items, key = { it.id }) { item -> PosterCard(item, onClick = { onOpenItem(item.id) }) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Delayed(modifier: Modifier = Modifier.fillMaxWidth().height(200.dp)) {
    if (rememberAfterDelay()) LoadingState(modifier)
}

@Composable
private fun PosterCard(item: ExtensionHomeItem, onClick: () -> Unit) {
    Column(Modifier.width(120.dp).clickable(onClick = onClick), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        MediaCover(item.poster, Modifier.fillMaxWidth(), contentDescription = item.title)
        Text(item.title, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun ResultRow(item: ExtensionHomeItem, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        MediaCover(item.poster, Modifier.width(56.dp), contentDescription = null)
        Text(item.title, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
    }
}

/**
 * `Route.ProviderItem(extensionId, sourceId, itemId)` (checklist ext.custom-providers.info / .watch / .read / .bookmark / .continue):
 * `providerInfo` → header, bookmark ("Guardar" / "Completado": `setProviderBookmark`), entries with filter and sort, "Continuar";
 * `watch` → [onPlay] with `PlayerArgs(media = item.match ?: MediaRef(), title, episode, provider = ProviderEntryRef(...))` (the player
 * resolves `providerWatch` and records history under `ext:<ext>:<source>:<item>`); `read` / `novel` → [onOpenReader] (`MangaReaderArgs`
 * with the extension coordinates); a `match` with AniList id → [onOpenDetails].
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun ProviderItemScreen(
    extensionId: String,
    sourceId: String,
    itemId: String,
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onOpenDetails: (anilistId: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val viewModel: ProviderItemViewModel = koinViewModel(
        key = "provider-item-$extensionId/$sourceId/$itemId",
        parameters = { parametersOf(extensionId, sourceId, itemId) },
    )
    val state by viewModel.state.collectAsStateWithLifecycle()
    val open: (ProviderEntry) -> Unit = { entry ->
        val watch = (entry.kind.ifBlank { state.kind }) == "watch"
        if (watch) viewModel.playerArgs(entry)?.let(onPlay) else viewModel.readerArgs(entry)?.let(onOpenReader)
    }
    Scaffold(
        modifier = modifier,
        topBar = {
            TopAppBar(
                title = {
                    Text(
                        (state.item as? LoadState.Ok)?.value?.title ?: stringResource(R.string.extensions_provider_title),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) }
                },
                actions = {
                    if (state.item is LoadState.Ok) {
                        val saved = state.bookmark != null
                        IconButton(onClick = viewModel::toggleBookmark) {
                            Icon(
                                if (saved) Icons.Outlined.Bookmark else Icons.Outlined.BookmarkBorder,
                                contentDescription = stringResource(if (saved) R.string.extensions_provider_unsave else R.string.extensions_provider_save),
                            )
                        }
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding), contentAlignment = Alignment.TopCenter) {
            when (val s = state.item) {
                LoadState.Loading -> Delayed(Modifier.fillMaxSize())
                is LoadState.Err -> ErrorState(
                    message = if ((s.cause as? AppError.Unknown)?.msg == ProviderItemViewModel.NOT_FOUND) {
                        stringResource(R.string.extensions_provider_item_missing)
                    } else {
                        s.cause.userMessage()
                    },
                    retryLabel = stringResource(UiR.string.ui_retry),
                    onRetry = viewModel::load,
                    title = stringResource(R.string.extensions_provider_error),
                )
                is LoadState.Ok -> {
                    val item = s.value
                    val entries = state.visibleEntries
                    LazyColumn(
                        modifier = Modifier.fillMaxSize().widthIn(max = 960.dp),
                        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 32.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        item(key = "header", contentType = "header") {
                            ItemHeader(
                                item = item,
                                kind = state.kind,
                                saved = state.bookmark != null,
                                completed = state.bookmark?.completed == true,
                                continueEntry = viewModel.continueEntry(),
                                onToggleBookmark = viewModel::toggleBookmark,
                                onToggleCompleted = viewModel::toggleCompleted,
                                onStart = { (viewModel.continueEntry() ?: filterAndSort(item.entries, "", EntrySort.ASC).firstOrNull())?.let(open) },
                                onOpenDetails = item.match?.anilistId?.let { id -> { onOpenDetails(id) } },
                            )
                        }
                        item(key = "entries-head", contentType = "head") {
                            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                OutlinedTextField(
                                    value = state.filter,
                                    onValueChange = viewModel::setFilter,
                                    label = { Text(stringResource(R.string.extensions_provider_filter)) },
                                    singleLine = true,
                                    modifier = Modifier.weight(1f),
                                )
                                IconButton(onClick = viewModel::toggleSort) {
                                    Icon(
                                        Icons.AutoMirrored.Outlined.Sort,
                                        contentDescription = stringResource(
                                            if (state.sort == EntrySort.ASC) R.string.extensions_provider_sort_desc else R.string.extensions_provider_sort_asc,
                                        ),
                                    )
                                }
                            }
                        }
                        if (item.entries.isEmpty()) {
                            item(key = "entries-empty", contentType = "state") {
                                EmptyState(icon = Icons.Outlined.Hub, title = stringResource(R.string.extensions_provider_no_entries))
                            }
                        } else if (entries.isEmpty()) {
                            item(key = "entries-filtered", contentType = "state") {
                                Text(stringResource(R.string.extensions_provider_no_match), color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        } else {
                            items(entries, key = { "e:${it.id}" }, contentType = { "entry" }) { e ->
                                val episode = entryEpisode(item.entries, e)
                                val progress = state.progress[episode]
                                EntryRow(e, kind = e.kind.ifBlank { state.kind }, watched = progress?.completed == true, inProgress = progress != null && !progress.completed, onClick = { open(e) })
                            }
                        }
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun ItemHeader(
    item: ProviderItem,
    kind: String,
    saved: Boolean,
    completed: Boolean,
    continueEntry: ProviderEntry?,
    onToggleBookmark: () -> Unit,
    onToggleCompleted: () -> Unit,
    onStart: () -> Unit,
    onOpenDetails: (() -> Unit)?,
) {
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
            MediaCover(item.poster, Modifier.width(120.dp), contentDescription = item.title)
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(item.title, style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
                FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Pill(stringResource(providerKindRes(kind)), tone = PillTone.NEUTRAL)
                    item.status?.takeIf { it.isNotBlank() }?.let { Pill(it, tone = PillTone.NEUTRAL) }
                    if (completed) Pill(stringResource(R.string.extensions_provider_completed), tone = PillTone.OK)
                }
                if (item.genres.isNotEmpty()) {
                    Text(item.genres.joinToString(" · "), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (item.entries.isNotEmpty()) {
                Button(onClick = onStart) {
                    Icon(if (kind == "watch") Icons.Outlined.PlayArrow else Icons.AutoMirrored.Outlined.MenuBook, contentDescription = null, modifier = Modifier.size(18.dp))
                    val label = continueEntry?.let { stringResource(R.string.extensions_provider_continue, it.number?.let(::formatNumber) ?: (it.title ?: "")) }
                        ?: stringResource(if (kind == "watch") R.string.extensions_provider_start_watch else R.string.extensions_provider_start_read)
                    Text(label, modifier = Modifier.padding(start = 8.dp))
                }
            }
            OutlinedButton(onClick = onToggleBookmark) {
                Icon(if (saved) Icons.Outlined.Bookmark else Icons.Outlined.BookmarkBorder, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(if (saved) R.string.extensions_provider_unsave else R.string.extensions_provider_save), modifier = Modifier.padding(start = 8.dp))
            }
            FilterChip(
                selected = completed,
                onClick = onToggleCompleted,
                label = { Text(stringResource(R.string.extensions_provider_completed)) },
                leadingIcon = if (completed) {
                    { Icon(Icons.Outlined.CheckCircle, contentDescription = null, modifier = Modifier.size(18.dp)) }
                } else {
                    null
                },
            )
            if (onOpenDetails != null) {
                OutlinedButton(onClick = onOpenDetails) {
                    Icon(Icons.AutoMirrored.Outlined.OpenInNew, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.extensions_provider_open_details), modifier = Modifier.padding(start = 8.dp))
                }
            }
        }
        item.description?.takeIf { it.isNotBlank() }?.let {
            Text(it, style = MaterialTheme.typography.bodyMedium, maxLines = 8, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun EntryRow(entry: ProviderEntry, kind: String, watched: Boolean, inProgress: Boolean, onClick: () -> Unit) {
    val number = entry.number?.let(::formatNumber)
    val headline = when {
        number != null && kind == "watch" -> stringResource(R.string.extensions_provider_episode, number)
        number != null -> stringResource(UiR.string.chapter_number, number)
        else -> entry.title ?: entry.id
    }
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = if (watched) MaterialTheme.colorScheme.surfaceContainerLow else MaterialTheme.colorScheme.surfaceContainer,
        modifier = Modifier.fillMaxWidth().clickable(onClick = onClick),
    ) {
        ListItem(
            headlineContent = { Text(headline, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            supportingContent = {
                val sub = listOfNotNull(entry.title?.takeIf { number != null && it.isNotBlank() }, entry.lang?.uppercase()).joinToString(" · ")
                if (sub.isNotEmpty()) Text(sub, maxLines = 2, overflow = TextOverflow.Ellipsis)
            },
            trailingContent = {
                when {
                    watched -> Icon(Icons.Outlined.CheckCircle, contentDescription = stringResource(R.string.extensions_provider_watched), tint = MaterialTheme.colorScheme.primary)
                    inProgress -> Pill(stringResource(R.string.extensions_provider_in_progress), tone = PillTone.WARN)
                    else -> Icon(if (kind == "watch") Icons.Outlined.PlayArrow else Icons.AutoMirrored.Outlined.MenuBook, contentDescription = null)
                }
            },
        )
    }
}

/**
 * Settings › Reproductor › Fuentes, mounted by the contract after the source rows of `PlayerSection` (checklist
 * sources.embed-sniffer.timeout-setting, sources.embed-sniffer.ranking): switch "Fuentes embed" (`SourcePrefs.allowEmbed`) and the
 * slider of `SourcePrefs.embedTimeoutSecs` (5..60 s).
 */
@Composable
fun SourcesSettingsExtras(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit, modifier: Modifier = Modifier) {
    val prefs = settings.sourcePrefs
    val secs = prefs.embedTimeoutSecs.coerceIn(SourcePrefs.EMBED_TIMEOUT_MIN_SECS, SourcePrefs.EMBED_TIMEOUT_MAX_SECS)
    var drag by remember(secs) { mutableFloatStateOf(secs.toFloat()) }
    Column(modifier.fillMaxWidth()) {
        ListItem(
            headlineContent = { Text(stringResource(R.string.extensions_embed_allow)) },
            supportingContent = { Text(stringResource(R.string.extensions_embed_allow_hint)) },
            trailingContent = {
                Switch(
                    checked = prefs.allowEmbed,
                    onCheckedChange = { on -> onUpdate { it.copy(sourcePrefs = it.sourcePrefs.copy(allowEmbed = on)) } },
                )
            },
        )
        ListItem(
            headlineContent = { Text(stringResource(R.string.extensions_embed_timeout, drag.roundToInt())) },
            supportingContent = {
                Column {
                    Text(stringResource(R.string.extensions_embed_timeout_hint))
                    Slider(
                        value = drag,
                        onValueChange = { drag = it },
                        onValueChangeFinished = {
                            val v = drag.roundToInt().coerceIn(SourcePrefs.EMBED_TIMEOUT_MIN_SECS, SourcePrefs.EMBED_TIMEOUT_MAX_SECS)
                            onUpdate { it.copy(sourcePrefs = it.sourcePrefs.copy(embedTimeoutSecs = v).sanitized()) }
                        },
                        valueRange = SourcePrefs.EMBED_TIMEOUT_MIN_SECS.toFloat()..SourcePrefs.EMBED_TIMEOUT_MAX_SECS.toFloat(),
                        steps = (SourcePrefs.EMBED_TIMEOUT_MAX_SECS - SourcePrefs.EMBED_TIMEOUT_MIN_SECS) / 5 - 1,
                        enabled = prefs.allowEmbed,
                    )
                }
            },
        )
    }
}
