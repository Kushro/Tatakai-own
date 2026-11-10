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
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ChecklistRtl
import androidx.compose.material.icons.outlined.Close
import androidx.compose.material.icons.outlined.DeleteSweep
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.DownloadDone
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material.icons.outlined.Refresh
import androidx.compose.material.icons.outlined.SwapVert
import androidx.compose.material.icons.outlined.Translate
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
import androidx.compose.material3.SuggestionChip
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
import androidx.compose.ui.draw.blur
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.manga.ChapterLists
import app.tatakai.next.manga.ChapterOrder
import app.tatakai.next.manga.ChapterText
import app.tatakai.next.manga.MangaInfo
import app.tatakai.next.manga.MangaRepositoryImpl
import app.tatakai.next.manga.key
import app.tatakai.next.model.ListProgress
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MangaChapter
import app.tatakai.next.model.MangaChapterSource
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.ReadEntry
import app.tatakai.next.ui.labels.readLabelRes
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.alternativeTitle
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import kotlinx.coroutines.launch
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf
import java.text.DateFormat
import java.util.Date
import java.util.Locale
import app.tatakai.next.ui.R as UiR

/**
 * `Route.Details(MediaKind.MANGA, anilistId)` (checklist manga.details / .anime-link / manga.metadata.* / manga.chapters.* /
 * manga.readlist / manga.download.chapters): cabecera, lista de lectura, sinopsis + metadatos (AniList + MangaUpdates), puntuaciones,
 * tags, personajes, recomendaciones, árbol de la serie, adaptación anime ([onOpenAnime]), capítulos por extensión con orden, filtro,
 * agrupación por volumen, Refrescar y Vaciar caché, selección múltiple → descarga.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun MangaDetailsScreen(
    anilistId: Int,
    onBack: () -> Unit,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onOpenAnime: (anilistId: Int) -> Unit,
    onOpenManga: (Media) -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenDownloads: () -> Unit,
    onOpenAccounts: () -> Unit,
    onOpenGenre: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: MangaDetailsViewModel = koinViewModel(key = "manga-$anilistId", parameters = { parametersOf(anilistId) })
    val ui by vm.ui.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val prefs by vm.prefs.collectAsStateWithLifecycle()
    val entry by vm.entry.collectAsStateWithLifecycle()
    val progress by vm.progress.collectAsStateWithLifecycle()
    val downloaded by vm.downloaded.collectAsStateWithLifecycle()
    val snackbar = remember { SnackbarHostState() }
    val scope = rememberCoroutineScope()
    var sourceSheet by rememberSaveable { mutableStateOf(false) }

    val queuedText = stringResource(R.string.manga_download_queued_snack)
    val openDownloadsLabel = stringResource(R.string.manga_open_downloads)
    val noChapterText = stringResource(R.string.manga_no_chapter)
    val errorPrefix = stringResource(UiR.string.ui_error_title)
    LaunchedEffect(vm) {
        vm.events.collect { ev ->
            when (ev) {
                is DetailsEvent.Open -> onOpenReader(ev.args)
                is DetailsEvent.Queued -> scope.launch {
                    val r = snackbar.showSnackbar(queuedText.format(ev.count), actionLabel = openDownloadsLabel)
                    if (r == androidx.compose.material3.SnackbarResult.ActionPerformed) onOpenDownloads()
                }
                is DetailsEvent.Failed -> scope.launch { snackbar.showSnackbar(errorPrefix) }
                DetailsEvent.NoChapter -> scope.launch { snackbar.showSnackbar(noChapterText) }
            }
        }
    }

    Scaffold(
        modifier = modifier,
        snackbarHost = { SnackbarHost(snackbar) },
        topBar = {
            TopAppBar(
                title = {
                    val t = (ui.info as? LoadState.Ok)?.value?.media?.displayTitle(settings.mangaTitleLanguage) ?: stringResource(R.string.manga_title)
                    Text(t, maxLines = 1, overflow = TextOverflow.Ellipsis)
                },
                navigationIcon = { IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) } },
            )
        },
        bottomBar = {
            if (ui.selecting) {
                BottomAppBar {
                    Text(pluralStringResource(R.plurals.manga_selected_count, ui.selected.size, ui.selected.size), modifier = Modifier.padding(start = 16.dp).weight(1f))
                    TextButton(onClick = vm::selectAllShown) { Text(stringResource(R.string.manga_select_all)) }
                    Button(onClick = vm::downloadSelected, enabled = ui.selected.isNotEmpty(), modifier = Modifier.padding(end = 8.dp)) {
                        Icon(Icons.Outlined.Download, contentDescription = null)
                        Text(stringResource(R.string.manga_download), modifier = Modifier.padding(start = 6.dp))
                    }
                }
            }
        },
    ) { padding ->
        when (val info = ui.info) {
            LoadState.Loading -> if (rememberAfterDelay()) LoadingState(Modifier.padding(padding))
            is LoadState.Err -> ErrorBlock(info.cause, vm::load, Modifier.padding(padding))
            is LoadState.Ok -> {
                val needsWarning = info.value.isAdult && settings.matureContent == MatureContentMode.WARN && anilistId !in prefs.matureAccepted
                if (needsWarning) {
                    MatureWarningDialog(onConfirm = vm::acceptMature, onDismiss = onBack)
                } else {
                    DetailsBody(
                        info = info.value,
                        ui = ui,
                        entry = entry,
                        progressChapter = progress?.chapterNumber,
                        nextChapter = vm.nextChapterNumber(progress, entry, (ui.chapters as? LoadState.Ok)?.value.orEmpty()),
                        downloadedIds = downloaded.map { MangaRepositoryImpl.downloadId(it) }.toSet(),
                        blurCover = info.value.isAdult && settings.matureContent == MatureContentMode.BLUR,
                        titleLanguage = settings.mangaTitleLanguage,
                        padding = padding,
                        vm = vm,
                        onOpenReader = onOpenReader,
                        onOpenAnime = onOpenAnime,
                        onOpenManga = onOpenManga,
                        onOpenExtensions = onOpenExtensions,
                        onOpenGenre = onOpenGenre,
                        onOpenSourceSheet = { sourceSheet = true },
                    )
                }
            }
        }
    }

    if (sourceSheet) {
        SourceSheet(
            sources = (ui.sources as? LoadState.Ok)?.value.orEmpty(),
            current = ui.source,
            preferredKey = prefs.preferredSourceByMedia[MangaRepositoryImpl.mediaKey(app.tatakai.next.model.MediaRef(anilistId = anilistId))],
            languages = prefs.languages,
            onSelect = { vm.selectSource(it); sourceSheet = false },
            onPrefer = vm::setPreferredSource,
            onLanguages = vm::setLanguages,
            onOpenExtensions = onOpenExtensions,
            onDismiss = { sourceSheet = false },
        )
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun DetailsBody(
    info: MangaInfo,
    ui: MangaDetailsUi,
    entry: ReadEntry?,
    progressChapter: Double?,
    nextChapter: Double?,
    downloadedIds: Set<String>,
    blurCover: Boolean,
    titleLanguage: app.tatakai.next.model.TitleLanguage,
    padding: PaddingValues,
    vm: MangaDetailsViewModel,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onOpenAnime: (Int) -> Unit,
    onOpenManga: (Media) -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenGenre: (String) -> Unit,
    onOpenSourceSheet: () -> Unit,
) {
    val media = info.media
    val title = media.displayTitle(titleLanguage)
    // chapter marks follow the readlist progress (AniList when linked): ListProgress.isChapterRead
    val readUpTo = (entry?.progress ?: 0).toDouble()
    LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(top = padding.calculateTopPadding(), bottom = padding.calculateBottomPadding() + 24.dp)) {
        item(key = "header") { Header(info, title, media.alternativeTitle(title), blurCover) }
        item(key = "actions") {
            ActionsRow(
                entry = entry,
                total = info.chapters ?: ui.details?.chapters,
                opening = ui.opening,
                hasProgress = progressChapter != null || (entry?.progress ?: 0) > 0,
                nextChapter = nextChapter,
                onRead = vm::readNext,
                onStatus = vm::setStatus,
                onProgress = vm::setProgress,
            )
        }
        media.synopsis?.let { synopsis -> item(key = "synopsis") { Synopsis(synopsis) } }
        item(key = "facts") { Facts(info, ui) }
        if (media.genres.isNotEmpty() || info.tags.isNotEmpty()) {
            item(key = "tags") {
                var spoilers by rememberSaveable { mutableStateOf(false) }
                SectionHeader(stringResource(R.string.manga_tags))
                FlowRow(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    media.genres.forEach { g -> AssistChip(onClick = { onOpenGenre(g) }, label = { Text(g) }) }
                    info.tags.filter { spoilers || !it.isSpoiler }.forEach { t ->
                        SuggestionChip(onClick = {}, label = { Text(t.rank?.let { r -> "${t.name} $r%" } ?: t.name) })
                    }
                    if (info.tags.any { it.isSpoiler }) {
                        TextButton(onClick = { spoilers = !spoilers }) {
                            Text(stringResource(if (spoilers) R.string.manga_hide_spoilers else R.string.manga_show_spoilers))
                        }
                    }
                }
            }
        }
        // Anime adaptation (checklist manga.details.anime-link).
        val adaptation = info.relations.firstOrNull { it.media.kind == MediaKind.ANIME && it.relation == "ADAPTATION" }
            ?: info.relations.firstOrNull { it.media.kind == MediaKind.ANIME }
        adaptation?.media?.ref?.anilistId?.let { animeId ->
            item(key = "adaptation") {
                ListItem(
                    leadingContent = { MediaCover(url = adaptation.media.cover, modifier = Modifier.width(48.dp)) },
                    overlineContent = { Text(stringResource(R.string.manga_anime_adaptation)) },
                    headlineContent = { Text(adaptation.media.displayTitle(titleLanguage)) },
                    supportingContent = { Text(listOfNotNull(adaptation.media.year?.toString(), anilistStatusLabel(adaptation.media.status)).joinToString(" · ")) },
                    trailingContent = { Icon(Icons.Outlined.Movie, contentDescription = null) },
                    modifier = Modifier.clickable { onOpenAnime(animeId) },
                )
            }
        }
        // Series tree (relations).
        val tree = info.relations.filter { it !== adaptation }
        if (tree.isNotEmpty()) {
            item(key = "relations") {
                SectionHeader(stringResource(R.string.manga_series_tree))
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    items(tree, key = { "${it.relation}-${it.media.ref.anilistId}" }) { rel ->
                        MangaCard(
                            rel.media,
                            titleLanguage,
                            onClick = {
                                val id = rel.media.ref.anilistId
                                if (rel.media.kind == MediaKind.ANIME && id != null) onOpenAnime(id) else onOpenManga(rel.media)
                            },
                            subtitle = relationLabel(rel.relation),
                        )
                    }
                }
            }
        }
        if (info.characters.isNotEmpty()) {
            item(key = "characters") {
                SectionHeader(stringResource(R.string.manga_characters))
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    items(info.characters, key = { it.character.id }) { c ->
                        Column(Modifier.width(88.dp)) {
                            MediaCover(url = c.character.image, modifier = Modifier.fillMaxWidth(), shape = MaterialTheme.shapes.large)
                            Text(c.character.name, style = MaterialTheme.typography.labelMedium, maxLines = 2, modifier = Modifier.padding(top = 4.dp))
                            c.role?.let { Text(it.lowercase().replaceFirstChar(Char::uppercase), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant) }
                        }
                    }
                }
            }
        }
        if (info.recommendations.isNotEmpty()) {
            item(key = "recs") {
                SectionHeader(stringResource(R.string.manga_recommendations))
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    items(info.recommendations, key = { it.media.ref.anilistId ?: it.media.title.hashCode() }) { r ->
                        MangaCard(r.media, titleLanguage, onClick = { onOpenManga(r.media) })
                    }
                }
            }
        }
        chaptersSection(ui, readUpTo, downloadedIds, vm, onOpenReader, onOpenExtensions, onOpenSourceSheet)
    }
}

@Composable
private fun Header(info: MangaInfo, title: String, alt: String?, blurCover: Boolean) {
    val media = info.media
    // MatureContentMode.BLUR: banner and cover stay blurred until tapped once.
    var revealed by remember(media.ref) { mutableStateOf(false) }
    val hide = blurCover && !revealed
    val blur = if (hide) Modifier.blur(24.dp) else Modifier
    Box(Modifier.fillMaxWidth()) {
        AsyncImage(
            model = media.banner ?: media.cover,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            modifier = Modifier.fillMaxWidth().height(180.dp).then(blur),
        )
        Box(Modifier.fillMaxWidth().height(180.dp).background(Brush.verticalGradient(listOf(Color.Transparent, MaterialTheme.colorScheme.surface))))
        Row(Modifier.padding(start = 16.dp, end = 16.dp, top = 96.dp), verticalAlignment = Alignment.Bottom) {
            Box(Modifier.width(110.dp).clickable(enabled = hide) { revealed = true }, contentAlignment = Alignment.Center) {
                MediaCover(url = media.cover, modifier = Modifier.fillMaxWidth().then(blur))
                if (hide) Text(stringResource(R.string.manga_mature_tap), style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(6.dp))
            }
            Column(Modifier.padding(start = 12.dp).weight(1f)) {
                Text(title, style = MaterialTheme.typography.headlineSmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
                alt?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1) }
                Row(Modifier.horizontalScroll(rememberScrollState()).padding(top = 6.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    formatLabel(info.format)?.let { Pill(it) }
                    anilistStatusLabel(info.status)?.let { Pill(it) }
                    originLabel(info.origin)?.let { Pill(it) }
                    media.year?.let { Pill(it.toString()) }
                    if (info.isAdult) Pill("18+")
                }
            }
        }
    }
}

@Composable
private fun ActionsRow(
    entry: ReadEntry?,
    total: Int?,
    opening: Boolean,
    hasProgress: Boolean,
    onRead: () -> Unit,
    onStatus: (ListStatus?) -> Unit,
    onProgress: (Int) -> Unit,
    nextChapter: Double? = null,
) {
    var statusMenu by remember { mutableStateOf(false) }
    Column(Modifier.padding(horizontal = 16.dp, vertical = 12.dp)) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            Button(onClick = onRead, enabled = !opening) {
                if (opening) CircularProgressIndicator(Modifier.size(18.dp), strokeWidth = 2.dp)
                val n = ChapterText.formatNumber(nextChapter)
                val label = when {
                    n == null -> stringResource(if (hasProgress) R.string.manga_continue else R.string.manga_read)
                    hasProgress -> stringResource(R.string.manga_continue_chapter, n)
                    else -> stringResource(R.string.manga_read_chapter, n)
                }
                Text(label, modifier = Modifier.padding(start = 6.dp))
            }
            Box {
                FilledTonalButton(onClick = { statusMenu = true }) {
                    Text(entry?.status?.let { stringResource(it.readLabelRes()) } ?: stringResource(R.string.manga_add_to_list))
                }
                DropdownMenu(expanded = statusMenu, onDismissRequest = { statusMenu = false }) {
                    READ_STATUSES.forEach { s ->
                        DropdownMenuItem(
                            text = { Text(stringResource(s.readLabelRes())) },
                            leadingIcon = if (entry?.status == s) ({ Icon(Icons.Outlined.CheckCircle, contentDescription = null) }) else null,
                            onClick = {
                                statusMenu = false
                                onStatus(s)
                            },
                        )
                    }
                    if (entry != null) {
                        HorizontalDivider()
                        DropdownMenuItem(text = { Text(stringResource(UiR.string.list_remove)) }, onClick = {
                            statusMenu = false
                            onStatus(null)
                        })
                    }
                }
            }
        }
        if (entry != null) {
            Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 8.dp)) {
                Text(stringResource(R.string.manga_progress_label, entry.progress, total?.toString() ?: "?"), modifier = Modifier.weight(1f))
                OutlinedButton(onClick = { onProgress(entry.progress - 1) }, enabled = entry.progress > 0) { Text("−") }
                Spacer(Modifier.width(8.dp))
                OutlinedButton(onClick = { onProgress(entry.progress + 1) }) { Text("+") }
            }
        }
    }
}

internal val READ_STATUSES = listOf(ListStatus.WATCHING, ListStatus.PLANNING, ListStatus.COMPLETED, ListStatus.PAUSED, ListStatus.DROPPED, ListStatus.REPEATING)

@Composable
private fun Synopsis(text: String) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    Column(Modifier.padding(horizontal = 16.dp).clickable { expanded = !expanded }) {
        Text(text, style = MaterialTheme.typography.bodyMedium, maxLines = if (expanded) Int.MAX_VALUE else 5, overflow = TextOverflow.Ellipsis)
        Text(stringResource(if (expanded) R.string.manga_less else R.string.manga_more), color = MaterialTheme.colorScheme.primary, style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(top = 4.dp))
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Facts(info: MangaInfo, ui: MangaDetailsUi) {
    val mu = ui.mangaUpdates
    val d = ui.details
    SectionCard(Modifier.padding(16.dp), title = stringResource(R.string.manga_info)) {
        @Composable
        fun Fact(label: String, value: String?) {
            if (value.isNullOrBlank()) return
            Row(Modifier.fillMaxWidth().padding(vertical = 3.dp)) {
                Text(label, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.width(130.dp))
                Text(value, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
            }
        }
        Fact(stringResource(R.string.manga_fact_authors), info.authors.joinToString(", ") { a -> a.role?.let { "${a.staff.name} ($it)" } ?: a.staff.name })
        Fact(stringResource(R.string.manga_fact_chapters), (info.chapters ?: d?.chapters ?: mu?.latestChapter)?.toString())
        Fact(stringResource(R.string.manga_fact_volumes), (info.volumes ?: d?.volumes)?.toString())
        Fact(stringResource(R.string.manga_fact_status), anilistStatusLabel(info.status))
        Fact(stringResource(R.string.manga_fact_publication), mu?.status)
        Fact(stringResource(R.string.manga_fact_publishers), mu?.publishers?.joinToString(", "))
        Fact(stringResource(R.string.manga_fact_serialization), mu?.serialization)
        Fact(stringResource(R.string.manga_fact_licensed), mu?.let { stringResource(if (it.licensed) R.string.manga_yes else R.string.manga_no) })
        Fact(stringResource(R.string.manga_fact_start), info.startDate)
        Fact(stringResource(R.string.manga_fact_end), info.endDate)
        Fact(stringResource(R.string.manga_fact_origin), originLabel(info.origin))
        Fact(stringResource(R.string.manga_fact_source), info.source?.lowercase()?.replace('_', ' ')?.replaceFirstChar(Char::uppercase))
        // Scores (checklist manga.details: puntuaciones).
        Row(Modifier.padding(top = 12.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            info.media.score?.let { ScoreBox(stringResource(R.string.manga_score_anilist), "$it%") }
            info.meanScore?.let { ScoreBox(stringResource(R.string.manga_score_mean), "$it%") }
            mu?.rating?.let { ScoreBox(stringResource(R.string.manga_score_mangaupdates), "%.1f".format(Locale.ROOT, it / 10.0)) }
            info.popularity?.let { ScoreBox(stringResource(R.string.manga_score_popularity), it.toString()) }
            info.favourites?.let { ScoreBox(stringResource(R.string.manga_score_favourites), it.toString()) }
        }
        if (info.rankings.isNotEmpty()) {
            FlowRow(Modifier.padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(6.dp)) { info.rankings.take(4).forEach { Pill(it) } }
        }
    }
}

@Composable
private fun ScoreBox(label: String, value: String) {
    Column(Modifier.background(MaterialTheme.colorScheme.surfaceContainerHigh, MaterialTheme.shapes.medium).padding(horizontal = 10.dp, vertical = 6.dp)) {
        Text(value, style = MaterialTheme.typography.titleMedium)
        Text(label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
    }
}

@Composable
private fun relationLabel(relation: String): String = when (relation.uppercase()) {
    "PREQUEL" -> stringResource(R.string.manga_rel_prequel)
    "SEQUEL" -> stringResource(R.string.manga_rel_sequel)
    "SIDE_STORY" -> stringResource(R.string.manga_rel_side_story)
    "SPIN_OFF" -> stringResource(R.string.manga_rel_spin_off)
    "ALTERNATIVE" -> stringResource(R.string.manga_rel_alternative)
    "PARENT" -> stringResource(R.string.manga_rel_parent)
    "ADAPTATION" -> stringResource(R.string.manga_rel_adaptation)
    "SOURCE" -> stringResource(R.string.manga_rel_source)
    else -> stringResource(R.string.manga_rel_other)
}

// ------------------------------------------------------------------------------------------------------------------- chapters

private fun LazyListScope.chaptersSection(
    ui: MangaDetailsUi,
    readUpTo: Double,
    downloadedIds: Set<String>,
    vm: MangaDetailsViewModel,
    onOpenReader: (MangaReaderArgs) -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenSourceSheet: () -> Unit,
) {
    item(key = "chapters-header") {
        SectionHeader(stringResource(R.string.manga_chapters)) {
            Row {
                IconButton(onClick = vm::refreshChapters) { Icon(Icons.Outlined.Refresh, contentDescription = stringResource(R.string.manga_refresh)) }
                IconButton(onClick = vm::clearCache) { Icon(Icons.Outlined.DeleteSweep, contentDescription = stringResource(R.string.manga_clear_cache)) }
                IconButton(onClick = { vm.setSelecting(!ui.selecting) }) {
                    Icon(if (ui.selecting) Icons.Outlined.Close else Icons.Outlined.ChecklistRtl, contentDescription = stringResource(R.string.manga_select_chapters))
                }
            }
        }
    }
    when (val sources = ui.sources) {
        LoadState.Loading -> item(key = "sources-loading") { Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() } }
        is LoadState.Err -> item(key = "sources-error") { ErrorBlock(sources.cause, vm::loadSources) }
        is LoadState.Ok -> {
            if (sources.value.isEmpty()) {
                item(key = "no-sources") {
                    EmptyState(
                        icon = Icons.Outlined.Extension,
                        title = stringResource(R.string.manga_no_sources),
                        message = stringResource(R.string.manga_no_sources_message),
                        actionLabel = stringResource(R.string.manga_open_extensions),
                        onAction = onOpenExtensions,
                    )
                }
                return
            }
            item(key = "sources") {
                Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    sources.value.forEach { s ->
                        FilterChip(
                            selected = ui.source?.key == s.key,
                            onClick = { vm.selectSource(s) },
                            enabled = s.itemId != null,
                            label = { Text(sourceChipLabel(s)) },
                        )
                    }
                    AssistChip(onClick = onOpenSourceSheet, label = { Text(stringResource(R.string.manga_source_settings)) }, leadingIcon = { Icon(Icons.Outlined.Translate, contentDescription = null) })
                }
            }
            item(key = "tools") { ChapterToolbar(ui, vm) }
            when (val chapters = ui.chapters) {
                LoadState.Loading -> item(key = "chapters-loading") { Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() } }
                is LoadState.Err -> item(key = "chapters-error") { ErrorBlock(chapters.cause, vm::refreshChapters) }
                is LoadState.Ok -> {
                    val shown = ui.shown
                    if (ui.source?.itemId == null) {
                        item(key = "unmatched") { Text(stringResource(R.string.manga_source_unmatched), modifier = Modifier.padding(16.dp)) }
                    } else if (shown.isEmpty()) {
                        item(key = "chapters-empty") { Text(stringResource(R.string.manga_chapters_empty), modifier = Modifier.padding(16.dp)) }
                    } else if (ui.tools.byVolume) {
                        ChapterLists.byVolume(shown).forEach { (volume, list) ->
                            item(key = "vol-$volume") {
                                Text(
                                    volume?.let { stringResource(R.string.manga_volume_n, it) } ?: stringResource(R.string.manga_volume_none),
                                    style = MaterialTheme.typography.titleSmall,
                                    color = MaterialTheme.colorScheme.primary,
                                    modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
                                )
                            }
                            items(list, key = { "c-" + MangaRepositoryImpl.downloadId(it) }) { ch -> ChapterRow(ch, ui, readUpTo, downloadedIds, vm, onOpenReader) }
                        }
                    } else {
                        items(shown, key = { "c-" + MangaRepositoryImpl.downloadId(it) }) { ch -> ChapterRow(ch, ui, readUpTo, downloadedIds, vm, onOpenReader) }
                    }
                }
            }
        }
    }
}

@Composable
private fun sourceChipLabel(s: MangaChapterSource): String = buildString {
    append(s.extensionName)
    s.chapterCount?.let { append(" · ").append(it) }
    if (s.languages.isNotEmpty()) append(" · ").append(s.languages.take(3).joinToString("/").uppercase())
    append(" · ").append((s.reliability * 100).toInt()).append('%')
}

@Composable
private fun ChapterToolbar(ui: MangaDetailsUi, vm: MangaDetailsViewModel) {
    val all = (ui.chapters as? LoadState.Ok)?.value.orEmpty()
    val langs = ChapterLists.languages(all)
    Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
        OutlinedTextField(
            value = ui.tools.query,
            onValueChange = { q -> vm.setTools { it.copy(query = q) } },
            singleLine = true,
            placeholder = { Text(stringResource(R.string.manga_chapter_filter_hint)) },
            modifier = Modifier.fillMaxWidth(),
        )
        Row(Modifier.horizontalScroll(rememberScrollState()).padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            AssistChip(
                onClick = { vm.setTools { it.copy(order = if (it.order == ChapterOrder.ASC) ChapterOrder.DESC else ChapterOrder.ASC) } },
                label = { Text(stringResource(if (ui.tools.order == ChapterOrder.ASC) R.string.manga_order_asc else R.string.manga_order_desc)) },
                leadingIcon = { Icon(Icons.Outlined.SwapVert, contentDescription = null) },
            )
            FilterChip(selected = ui.tools.byVolume, onClick = { vm.setTools { it.copy(byVolume = !it.byVolume) } }, label = { Text(stringResource(R.string.manga_group_volume)) })
            if (langs.size > 1) {
                FilterDropdown(
                    label = stringResource(R.string.manga_chapter_language),
                    value = ui.tools.lang?.uppercase(),
                    options = listOf<String?>(null) + langs,
                    optionLabel = { it?.let { l -> Locale.forLanguageTag(l).getDisplayLanguage(Locale.getDefault()).ifBlank { l } } ?: stringResource(R.string.manga_filter_any) },
                    onSelect = { l -> vm.setTools { it.copy(lang = l) } },
                )
            }
            Text(stringResource(R.string.manga_chapter_count, ui.shown.size), style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun ChapterRow(
    ch: MangaChapter,
    ui: MangaDetailsUi,
    readUpTo: Double,
    downloadedIds: Set<String>,
    vm: MangaDetailsViewModel,
    onOpenReader: (MangaReaderArgs) -> Unit,
) {
    val id = MangaRepositoryImpl.downloadId(ch)
    val isDownloaded = ch.downloaded || id in downloadedIds
    val read = ListProgress.isChapterRead(ch.number, readUpTo.toInt())
    val date = ch.releasedAtMs?.let { remember(it) { DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(it)) } }
    ListItem(
        headlineContent = {
            Text(
                ChapterText.formatNumber(ch.number)?.let { stringResource(R.string.manga_chapter_n, it) + (ch.title?.takeIf { t -> t.isNotBlank() }?.let { t -> " · $t" } ?: "") } ?: (ch.title ?: ch.id),
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                color = if (read) MaterialTheme.colorScheme.onSurfaceVariant else MaterialTheme.colorScheme.onSurface,
            )
        },
        supportingContent = {
            Text(listOfNotNull(ch.group, ch.lang?.uppercase(), date, ch.volume?.let { stringResource(R.string.manga_volume_n, it) }).joinToString(" · "))
        },
        leadingContent = if (ui.selecting) ({ Checkbox(checked = id in ui.selected, onCheckedChange = { vm.toggle(ch) }) }) else null,
        trailingContent = {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (isDownloaded) Icon(Icons.Outlined.DownloadDone, contentDescription = stringResource(R.string.manga_downloaded))
                val label = ChapterText.formatNumber(ch.number)
                if (!ui.selecting && label != null) {
                    IconButton(onClick = { vm.toggleRead(ch) }) {
                        Icon(
                            if (read) Icons.Filled.CheckCircle else Icons.Outlined.CheckCircle,
                            contentDescription = stringResource(if (read) R.string.manga_mark_unread else R.string.manga_mark_read, label),
                            tint = if (read) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
            }
        },
        modifier = Modifier.clickable {
            if (ui.selecting) vm.toggle(ch) else onOpenReader(vm.argsFor(ch.copy(downloaded = isDownloaded)))
        },
    )
}

// ------------------------------------------------------------------------------------------------------------------- source sheet

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SourceSheet(
    sources: List<MangaChapterSource>,
    current: MangaChapterSource?,
    preferredKey: String?,
    languages: List<String>,
    onSelect: (MangaChapterSource) -> Unit,
    onPrefer: (MangaChapterSource?) -> Unit,
    onLanguages: (List<String>) -> Unit,
    onOpenExtensions: () -> Unit,
    onDismiss: () -> Unit,
) {
    AdaptiveSheet(onDismissRequest = onDismiss) {
        Text(stringResource(R.string.manga_source_settings), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(16.dp))
        Text(stringResource(R.string.manga_preferred_languages), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(horizontal = 16.dp))
        Text(stringResource(R.string.manga_preferred_languages_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp))
        FlowRow(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            LANGUAGE_CHOICES.forEach { code ->
                val selected = code in languages
                FilterChip(
                    selected = selected,
                    onClick = { onLanguages(if (selected) languages - code else languages + code) },
                    label = { Text(Locale.forLanguageTag(code).getDisplayLanguage(Locale.getDefault()).replaceFirstChar(Char::uppercase)) },
                )
            }
        }
        HorizontalDivider()
        Text(stringResource(R.string.manga_preferred_source), style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(16.dp))
        ListItem(
            headlineContent = { Text(stringResource(R.string.manga_source_auto)) },
            supportingContent = { Text(stringResource(R.string.manga_source_auto_hint)) },
            trailingContent = { if (preferredKey == null) Icon(Icons.Outlined.CheckCircle, contentDescription = null) },
            modifier = Modifier.clickable { onPrefer(null) },
        )
        sources.forEach { s ->
            ListItem(
                headlineContent = { Text(s.extensionName) },
                supportingContent = {
                    Text(
                        if (s.itemId == null) stringResource(R.string.manga_source_unmatched) else sourceChipLabel(s),
                    )
                },
                trailingContent = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (preferredKey == s.key) Icon(Icons.Outlined.CheckCircle, contentDescription = stringResource(R.string.manga_preferred_source))
                        TextButton(onClick = { onPrefer(s) }, enabled = s.itemId != null) { Text(stringResource(R.string.manga_prefer)) }
                    }
                },
                modifier = Modifier.clickable(enabled = s.itemId != null) { onSelect(s) }.then(
                    if (current?.key == s.key) Modifier.background(MaterialTheme.colorScheme.secondaryContainer.copy(alpha = 0.4f)) else Modifier,
                ),
            )
        }
        TextButton(onClick = onOpenExtensions, modifier = Modifier.padding(horizontal = 8.dp)) { Text(stringResource(R.string.manga_open_extensions)) }
    }
}

internal val LANGUAGE_CHOICES = listOf("en", "es", "pt", "fr", "de", "it", "ru", "id", "vi", "th", "ja", "ko", "zh", "ar", "tr", "pl")
