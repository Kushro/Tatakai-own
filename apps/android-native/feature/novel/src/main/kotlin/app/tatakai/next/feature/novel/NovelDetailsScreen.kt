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
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ChecklistRtl
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.DownloadDone
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.FactCheck
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.SwapVert
import androidx.compose.material3.AssistChip
import androidx.compose.material3.BottomAppBar
import androidx.compose.material3.Button
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.SnackbarResult
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.manga.ChapterText
import app.tatakai.next.model.ListProgress
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.NovelChapter
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.NovelSourceSummary
import app.tatakai.next.novel.NovelChapters
import app.tatakai.next.novel.NovelRepositoryImpl
import app.tatakai.next.ui.labels.readLabelRes
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.alternativeTitle
import app.tatakai.next.ui.media.displayTitle
import coil3.compose.AsyncImage
import kotlinx.coroutines.launch
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf
import java.text.DateFormat
import java.util.Date
import java.util.Locale
import app.tatakai.next.ui.R as UiR

/**
 * `Route.NovelDetails(anilistId)` (checklist novel.details / .check-sources / novel.chapters / novel.readlist / novel.download.chapters):
 * sinopsis, metadatos, lista de lectura, fuentes por extensión con "Comprobar todas", filtro por traductor, capítulos con idioma y fecha
 * → [onOpenReader], selección múltiple → descarga.
 */
@OptIn(ExperimentalMaterial3Api::class, ExperimentalLayoutApi::class)
@Composable
fun NovelDetailsScreen(
    anilistId: Int,
    onBack: () -> Unit,
    onOpenReader: (NovelReaderArgs) -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenDownloads: () -> Unit,
    onOpenAccounts: () -> Unit,
    onOpenGenre: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: NovelDetailsViewModel = koinViewModel(key = "novel-$anilistId", parameters = { parametersOf(anilistId) })
    val ui by vm.ui.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val entry by vm.entry.collectAsStateWithLifecycle()
    val progress by vm.progress.collectAsStateWithLifecycle()
    val downloaded by vm.downloaded.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    val queued = stringResource(R.string.novel_download_queued_snack)
    val openDownloads = stringResource(R.string.novel_open_downloads)
    val noChapter = stringResource(R.string.novel_no_chapter)
    val failed = stringResource(UiR.string.ui_error_title)
    LaunchedEffect(vm) {
        vm.events.collect { ev ->
            when (ev) {
                is NovelEvent.Open -> onOpenReader(ev.args)
                is NovelEvent.Queued -> scope.launch {
                    if (snackbar.showSnackbar(queued.format(ev.count), actionLabel = openDownloads) == SnackbarResult.ActionPerformed) onOpenDownloads()
                }
                is NovelEvent.Failed -> scope.launch { snackbar.showSnackbar(failed) }
                NovelEvent.NoChapter -> scope.launch { snackbar.showSnackbar(noChapter) }
            }
        }
    }
    val lang = settings.mangaTitleLanguage
    Scaffold(
        modifier = modifier,
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                title = { Text((ui.info as? LoadState.Ok)?.value?.media?.displayTitle(lang) ?: stringResource(R.string.novel_title), maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) } },
            )
        },
        bottomBar = {
            if (ui.selecting) {
                BottomAppBar {
                    Text(pluralStringResource(R.plurals.novel_selected_count, ui.selected.size, ui.selected.size), modifier = Modifier.padding(start = 16.dp).weight(1f))
                    TextButton(onClick = vm::selectAll) { Text(stringResource(R.string.novel_select_all)) }
                    Button(onClick = vm::downloadSelected, enabled = ui.selected.isNotEmpty(), modifier = Modifier.padding(end = 8.dp)) {
                        Icon(Icons.Outlined.Download, contentDescription = null)
                        Text(stringResource(R.string.novel_download), modifier = Modifier.padding(start = 6.dp))
                    }
                }
            }
        },
    ) { padding ->
        when (val info = ui.info) {
            LoadState.Loading -> if (rememberAfterDelay()) LoadingState(Modifier.padding(padding))
            is LoadState.Err -> NovelError(info.cause, vm::load, Modifier.padding(padding))
            is LoadState.Ok -> {
                val media = info.value.media
                val title = media.displayTitle(lang)
                // chapter marks follow the readlist progress (AniList when linked): ListProgress.isChapterRead
                val listProgress = entry?.progress ?: 0
                val nextChapter = ChapterText.formatNumber(vm.nextChapterNumber(progress, entry, (ui.chapters as? LoadState.Ok)?.value.orEmpty()))
                val started = progress != null || listProgress > 0
                LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = padding.calculateBottomPadding() + 24.dp)) {
                    item(key = "header") {
                        Box(Modifier.fillMaxWidth()) {
                            AsyncImage(model = media.banner ?: media.cover, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxWidth().height(170.dp))
                            Box(Modifier.fillMaxWidth().height(170.dp).background(Brush.verticalGradient(listOf(Color.Transparent, MaterialTheme.colorScheme.surface))))
                            Row(Modifier.padding(start = 16.dp, end = 16.dp, top = 90.dp), verticalAlignment = Alignment.Bottom) {
                                MediaCover(url = media.cover, modifier = Modifier.width(110.dp))
                                Column(Modifier.padding(start = 12.dp).weight(1f)) {
                                    Text(title, style = MaterialTheme.typography.headlineSmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
                                    media.alternativeTitle(title)?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1) }
                                    Row(Modifier.horizontalScroll(rememberScrollState()).padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                        Pill(stringResource(R.string.novel_format_novel))
                                        novelStatusLabel(info.value.status)?.let { Pill(it) }
                                        novelOriginLabel(info.value.origin)?.let { Pill(it) }
                                        media.year?.let { Pill(it.toString()) }
                                    }
                                }
                            }
                        }
                    }
                    item(key = "actions") {
                        var menu by remember { mutableStateOf(false) }
                        Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
                            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                Button(onClick = vm::readNext) {
                                    Text(
                                        when {
                                            nextChapter == null -> stringResource(if (started) R.string.novel_continue else R.string.novel_read)
                                            started -> stringResource(R.string.novel_continue_chapter, nextChapter)
                                            else -> stringResource(R.string.novel_read_chapter, nextChapter)
                                        },
                                    )
                                }
                                Box {
                                    FilledTonalButton(onClick = { menu = true }) { Text(entry?.status?.let { stringResource(it.readLabelRes()) } ?: stringResource(R.string.novel_add_to_list)) }
                                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                                        NOVEL_READ_STATUSES.forEach { s ->
                                            DropdownMenuItem(
                                                text = { Text(stringResource(s.readLabelRes())) },
                                                leadingIcon = if (entry?.status == s) ({ Icon(Icons.Outlined.CheckCircle, contentDescription = null) }) else null,
                                                onClick = { menu = false; vm.setStatus(s) },
                                            )
                                        }
                                        if (entry != null) {
                                            HorizontalDivider()
                                            DropdownMenuItem(text = { Text(stringResource(UiR.string.list_remove)) }, onClick = { menu = false; vm.setStatus(null) })
                                        }
                                    }
                                }
                            }
                            entry?.let { e ->
                                Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 8.dp)) {
                                    Text(stringResource(R.string.novel_progress_label, e.progress, (e.totalChapters ?: info.value.chapters)?.toString() ?: "?"), modifier = Modifier.weight(1f))
                                    OutlinedButton(onClick = { vm.setProgress(e.progress - 1) }, enabled = e.progress > 0) { Text("−") }
                                    Spacer(Modifier.width(8.dp))
                                    OutlinedButton(onClick = { vm.setProgress(e.progress + 1) }) { Text("+") }
                                }
                            }
                        }
                    }
                    media.synopsis?.let { s ->
                        item(key = "synopsis") {
                            var expanded by rememberSaveable { mutableStateOf(false) }
                            Column(Modifier.padding(horizontal = 16.dp).clickable { expanded = !expanded }) {
                                Text(s, style = MaterialTheme.typography.bodyMedium, maxLines = if (expanded) Int.MAX_VALUE else 5, overflow = TextOverflow.Ellipsis)
                                Text(stringResource(if (expanded) R.string.novel_less else R.string.novel_more), color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge)
                            }
                        }
                    }
                    item(key = "facts") {
                        SectionCard(Modifier.padding(16.dp), title = stringResource(R.string.novel_info)) {
                            @Composable
                            fun Fact(label: String, value: String?) {
                                if (value.isNullOrBlank()) return
                                Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
                                    Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(120.dp))
                                    Text(value, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                                }
                            }
                            Fact(stringResource(R.string.novel_fact_authors), info.value.authors.joinToString(", ") { a -> a.role?.let { "${a.staff.name} ($it)" } ?: a.staff.name })
                            Fact(stringResource(R.string.novel_fact_volumes), info.value.volumes?.toString())
                            Fact(stringResource(R.string.novel_fact_chapters), info.value.chapters?.toString())
                            Fact(stringResource(R.string.novel_fact_status), novelStatusLabel(info.value.status))
                            Fact(stringResource(R.string.novel_fact_start), info.value.startDate)
                            Fact(stringResource(R.string.novel_fact_end), info.value.endDate)
                            Fact(stringResource(R.string.novel_fact_score), media.score?.let { "$it%" })
                            Fact(stringResource(R.string.novel_fact_origin), novelOriginLabel(info.value.origin))
                        }
                    }
                    if (media.genres.isNotEmpty()) {
                        item(key = "genres") {
                            FlowRow(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                                media.genres.forEach { g -> AssistChip(onClick = { onOpenGenre(g) }, label = { Text(g) }) }
                            }
                        }
                    }
                    // ---- sources & chapters ----
                    item(key = "chapters-header") {
                        NovelSectionHeader(stringResource(R.string.novel_chapters)) {
                            Row {
                                TextButton(onClick = vm::checkAll, enabled = !ui.checking) {
                                    if (ui.checking) CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp) else Icon(Icons.Outlined.FactCheck, contentDescription = null)
                                    Text(stringResource(R.string.novel_check_all), modifier = Modifier.padding(start = 6.dp))
                                }
                                IconButton(onClick = vm::refreshChapters) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.novel_refresh)) }
                                IconButton(onClick = { vm.setSelecting(!ui.selecting) }) {
                                    Icon(if (ui.selecting) Icons.Outlined.Close else Icons.Outlined.ChecklistRtl, contentDescription = stringResource(R.string.novel_select_chapters))
                                }
                            }
                        }
                    }
                    when (val sources = ui.sources) {
                        LoadState.Loading -> item(key = "src-loading") { Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() } }
                        is LoadState.Err -> item(key = "src-err") { NovelError(sources.cause, vm::loadSources) }
                        is LoadState.Ok -> if (sources.value.isEmpty()) {
                            item(key = "no-src") {
                                EmptyState(
                                    icon = Icons.Outlined.Extension,
                                    title = stringResource(R.string.novel_no_sources),
                                    message = stringResource(R.string.novel_no_sources_message),
                                    actionLabel = stringResource(R.string.novel_open_extensions),
                                    onAction = onOpenExtensions,
                                )
                            }
                        } else {
                            item(key = "sources") {
                                Column(Modifier.padding(horizontal = 8.dp)) {
                                    sources.value.forEach { s -> SourceRow(s, selected = ui.source?.extensionId == s.extensionId && ui.source?.sourceId == s.sourceId) { vm.selectSource(s) } }
                                }
                            }
                            item(key = "tools") { NovelChapterTools(ui, vm) }
                            when (val chapters = ui.chapters) {
                                LoadState.Loading -> item(key = "ch-loading") { Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() } }
                                is LoadState.Err -> item(key = "ch-err") { NovelError(chapters.cause, vm::refreshChapters) }
                                is LoadState.Ok -> {
                                    val shown = ui.shown
                                    if (shown.isEmpty()) {
                                        item(key = "ch-empty") { Text(stringResource(R.string.novel_chapters_empty), modifier = Modifier.padding(16.dp)) }
                                    }
                                    items(shown, key = { "c-" + NovelRepositoryImpl.downloadId(it) }) { ch ->
                                        val id = NovelRepositoryImpl.downloadId(ch)
                                        val isDownloaded = ch.downloaded || id in downloaded
                                        NovelChapterRow(ch, read = ListProgress.isChapterRead(ch.number, listProgress), downloaded = isDownloaded, selecting = ui.selecting, selected = id in ui.selected,
                                            onToggle = { vm.toggle(ch) }, onOpen = { onOpenReader(vm.argsFor(ch, offline = isDownloaded)) },
                                            onToggleRead = { vm.toggleRead(ch) })
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
private fun SourceRow(s: NovelSourceSummary, selected: Boolean, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(s.extensionName, color = if (selected) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface) },
        supportingContent = {
            Text(
                when {
                    s.itemId == null -> stringResource(R.string.novel_source_unmatched)
                    else -> listOfNotNull(
                        s.chapterCount?.let { stringResource(R.string.novel_chapter_count, it) } ?: stringResource(R.string.novel_source_unchecked),
                        s.languages.joinToString("/").uppercase().ifBlank { null },
                        s.translator,
                    ).joinToString(" · ")
                },
            )
        },
        trailingContent = { if (selected) Icon(Icons.Outlined.CheckCircle, contentDescription = null) },
        modifier = Modifier.clickable(enabled = s.itemId != null, onClick = onClick),
    )
}

@Composable
private fun NovelChapterTools(ui: NovelDetailsUi, vm: NovelDetailsViewModel) {
    val all = (ui.chapters as? LoadState.Ok)?.value.orEmpty()
    val translators = NovelChapters.translators(all)
    val langs = NovelChapters.languages(all)
    Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
        OutlinedTextField(value = ui.query, onValueChange = vm::setQuery, singleLine = true, placeholder = { Text(stringResource(R.string.novel_chapter_filter_hint)) }, modifier = Modifier.fillMaxWidth())
        Row(Modifier.horizontalScroll(rememberScrollState()).padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            AssistChip(
                onClick = vm::toggleOrder,
                label = { Text(stringResource(if (ui.ascending) R.string.novel_order_asc else R.string.novel_order_desc)) },
                leadingIcon = { Icon(Icons.Outlined.SwapVert, contentDescription = null) },
            )
            if (translators.isNotEmpty()) {
                NovelFilterDropdown(stringResource(R.string.novel_translator), ui.translator, listOf<String?>(null) + translators, optionLabel = { it ?: stringResource(R.string.novel_filter_any) }, onSelect = vm::setTranslator)
            }
            if (langs.size > 1) {
                NovelFilterDropdown(
                    stringResource(R.string.novel_language),
                    ui.lang?.uppercase(),
                    listOf<String?>(null) + langs,
                    optionLabel = { it?.let { l -> Locale.forLanguageTag(l).getDisplayLanguage(Locale.getDefault()).ifBlank { l } } ?: stringResource(R.string.novel_filter_any) },
                    onSelect = vm::setLang,
                )
            }
            Text(stringResource(R.string.novel_chapter_count, ui.shown.size), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun NovelChapterRow(
    ch: NovelChapter,
    read: Boolean,
    downloaded: Boolean,
    selecting: Boolean,
    selected: Boolean,
    onToggle: () -> Unit,
    onOpen: () -> Unit,
    onToggleRead: () -> Unit = {},
) {
    val date = ch.releasedAtMs?.let { remember(it) { DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(it)) } }
    ListItem(
        headlineContent = {
            Text(
                (ChapterText.formatNumber(ch.number)?.let { stringResource(R.string.novel_chapter_n, it) } ?: (ch.title ?: ch.id)) +
                    (if (ch.number != null) ch.title?.takeIf { it.isNotBlank() }?.let { " · $it" } ?: "" else ""),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                color = if (read) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
            )
        },
        supportingContent = { Text(listOfNotNull(ch.translator, ch.lang?.uppercase(), date).joinToString(" · ")) },
        leadingContent = if (selecting) ({ Checkbox(checked = selected, onCheckedChange = { onToggle() }) }) else null,
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (downloaded) Icon(Icons.Outlined.DownloadDone, contentDescription = stringResource(R.string.novel_downloaded))
                val label = ChapterText.formatNumber(ch.number)
                if (!selecting && label != null) {
                    IconButton(onClick = onToggleRead) {
                        Icon(
                            if (read) Icons.Filled.CheckCircle else Icons.Outlined.CheckCircle,
                            contentDescription = stringResource(if (read) R.string.novel_mark_unread else R.string.novel_mark_read, label),
                            tint = if (read) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        },
        modifier = Modifier.clickable { if (selecting) onToggle() else onOpen() },
    )
}
