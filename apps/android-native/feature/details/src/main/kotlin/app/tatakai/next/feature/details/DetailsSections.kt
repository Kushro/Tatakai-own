@file:OptIn(ExperimentalMaterial3ExpressiveApi::class, ExperimentalLayoutApi::class)

package app.tatakai.next.feature.details

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.Cast
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material.icons.outlined.Tune
import androidx.compose.material.icons.outlined.TravelExplore
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedIconButton
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.SplitButtonDefaults
import androidx.compose.material3.SplitButtonLayout
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.feature.player.sources.SourceRow
import app.tatakai.next.model.Episode
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.formatDuration
import app.tatakai.next.ui.media.MediaCard
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.progressFraction
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import java.util.Locale

/** The scrolling column of Details: header, (phone) source panel, tabs and the selected tab's content. */
internal fun LazyListScope.detailsItems(
    media: Media,
    state: DetailsUiState,
    actions: DetailsActions,
    tab: DetailsTab,
    onTab: (DetailsTab) -> Unit,
    range: Int,
    onRange: (Int) -> Unit,
    includePanel: Boolean,
) {
    item(key = "header", contentType = "header") { Header(media, state, actions) }
    if (includePanel) {
        item(key = "panel", contentType = "panel") {
            SourcePanelCard(state, actions, Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
        }
    }
    item(key = "tabs", contentType = "tabs") { DetailsTabs(tab, onTab) }
    when (tab) {
        DetailsTab.EPISODES -> episodeItems(state, actions, range, onRange)
        DetailsTab.INFO -> item(key = "info", contentType = "info") { InfoTab(media) }
        DetailsTab.RELATED -> item(key = "related", contentType = "related") { RelatedTab(media, state.related, actions) }
    }
}

// ---------------------------------------------------------------- header

@Composable
private fun Header(media: Media, state: DetailsUiState, actions: DetailsActions) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    val background = MaterialTheme.colorScheme.background
    Column(Modifier.fillMaxWidth()) {
        Box(Modifier.fillMaxWidth().height(220.dp)) {
            AsyncImage(
                model = media.banner ?: media.cover,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHighest),
            )
            Box(Modifier.fillMaxSize().background(Brush.verticalGradient(listOf(background.copy(alpha = 0f), background))))
        }
        Row(
            Modifier.padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            MediaCover(url = media.cover, modifier = Modifier.width(110.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(media.title, style = MaterialTheme.typography.headlineSmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
                alternativeTitle(media)?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2)
                }
            }
        }
        MetadataChips(media, actions.onOpenGenre)
        FlowRow(
            Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            WatchSplitButton(state, actions)
            OutlinedButton(onClick = actions.onSendToDevice, enabled = !state.sending) {
                Icon(Icons.Outlined.Cast, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.details_send_device), modifier = Modifier.padding(start = 6.dp))
            }
            DownloadMenuButton(state, actions, icon = Icons.Outlined.Download, outlined = true)
        }
        state.resumeEntry?.let { entry ->
            Text(
                stringResource(R.string.details_resume_at, formatDuration(entry.progressSec.toLong())),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp),
            )
        }
        val synopsis = media.synopsis?.takeIf { it.isNotBlank() }
        Text(
            synopsis ?: stringResource(R.string.details_no_synopsis),
            style = MaterialTheme.typography.bodyMedium,
            maxLines = if (expanded) Int.MAX_VALUE else SYNOPSIS_LINES,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(horizontal = 16.dp).padding(top = 8.dp),
        )
        if (synopsis != null && synopsis.length > SYNOPSIS_TOGGLE_CHARS) {
            TextButton(onClick = { expanded = !expanded }, modifier = Modifier.padding(horizontal = 8.dp)) {
                Text(stringResource(if (expanded) R.string.details_less else R.string.details_more))
            }
        }
    }
}

/** Split button Ver / Continuar ▾ (07 §3.4): the ▾ opens the source picker and picking starts playback with that source. */
@Composable
private fun WatchSplitButton(state: DetailsUiState, actions: DetailsActions) {
    SplitButtonLayout(
        leadingButton = {
            SplitButtonDefaults.LeadingButton(onClick = actions.onWatch) {
                Icon(Icons.Filled.PlayArrow, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(watchLabel(state), modifier = Modifier.padding(start = 6.dp))
            }
        },
        trailingButton = {
            SplitButtonDefaults.TrailingButton(
                onClick = { actions.onOpenPicker(true) },
                enabled = state.panel != null && state.hasExtensions,
            ) {
                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.details_choose_source))
            }
        },
    )
}

@Composable
private fun MetadataChips(media: Media, onGenre: (String) -> Unit) {
    val resources = LocalContext.current.resources
    FlowRow(
        Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        statusPill(media.status)?.let { (label, tone) -> Pill(label, tone = tone) }
        val season = listOfNotNull(media.year?.toString(), media.season?.let { seasonLabel(it) }).joinToString(" · ")
        if (season.isNotEmpty()) InfoChip(season)
        media.episodes?.let { InfoChip(resources.getQuantityString(R.plurals.details_episodes_count, it, it)) }
        media.score?.let { InfoChip(stringResource(R.string.details_score, formatScore(it)), icon = Icons.Filled.Star) }
        media.genres.forEach { genre ->
            AssistChip(onClick = { onGenre(genre) }, label = { Text(genre) })
        }
    }
}

/** Download icon with its menu: "Descargar episodio N" and, when the best torrent is a batch, "Descargar temporada". */
@Composable
internal fun DownloadMenuButton(state: DetailsUiState, actions: DetailsActions, icon: ImageVector, outlined: Boolean = false) {
    var open by remember { mutableStateOf(false) }
    val description = stringResource(R.string.details_download)
    Box {
        if (outlined) {
            OutlinedIconButton(onClick = { open = true }) { Icon(icon, contentDescription = description) }
        } else {
            IconButton(onClick = { open = true }) { Icon(icon, contentDescription = description) }
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            val panel = state.panel
            val best = state.bestTorrent
            if (panel != null && best != null) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.details_download_episode, panel.episode)) },
                    onClick = {
                        open = false
                        actions.onDownloadSource(best, panel.episode)
                    },
                    leadingIcon = { Icon(Icons.Outlined.Download, contentDescription = null) },
                )
                if (state.canDownloadSeason) {
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.details_download_season)) },
                        onClick = {
                            open = false
                            actions.onDownloadSeason()
                        },
                        leadingIcon = { Icon(Icons.Outlined.Movie, contentDescription = null) },
                    )
                }
            } else {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.details_download_none)) },
                    onClick = { open = false },
                    enabled = false,
                )
            }
        }
    }
}

// ---------------------------------------------------------------- source panel

/** sources.js `summary`: mode + Settings link, the chosen row (SourceRow of :feature:player), who chose it, "Ver todas (n)". */
@Composable
internal fun SourcePanelCard(state: DetailsUiState, actions: DetailsActions, modifier: Modifier = Modifier) {
    val panel = state.panel
    SectionCard(modifier = modifier, title = stringResource(R.string.details_src_title, panel?.episode ?: state.continueEpisode)) {
        Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
            val mode = stringResource(if (state.mode == SourceMode.MANUAL) UiR.string.src_mode_manual else UiR.string.src_mode_auto)
            Text(
                stringResource(R.string.details_src_mode, mode),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.weight(1f),
            )
            TextButton(onClick = { actions.onOpenSettings(SettingsSection.PLAYER) }) {
                Icon(Icons.Outlined.Tune, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.details_src_settings), modifier = Modifier.padding(start = 4.dp))
            }
        }
        when {
            !state.hasExtensions -> EmptyState(
                icon = Icons.Outlined.Extension,
                title = stringResource(R.string.details_no_extension_title),
                message = stringResource(R.string.details_no_extension_body),
                actionLabel = stringResource(R.string.details_go_extensions),
                onAction = actions.onOpenExtensions,
            )
            panel == null -> DelayedLoading()
            else -> when (val sources = panel.sources) {
                LoadState.Loading -> DelayedLoading()
                is LoadState.Err -> ErrorState(
                    message = sources.cause.userMessage(),
                    retryLabel = stringResource(UiR.string.ui_retry),
                    onRetry = actions.onRetrySources,
                    compact = true,
                )
                is LoadState.Ok -> {
                    val shown = state.shownSource
                    if (shown == null) {
                        EmptyState(
                            icon = Icons.Outlined.Extension,
                            title = stringResource(R.string.details_no_sources_title),
                            message = stringResource(R.string.details_no_sources_message),
                            actionLabel = stringResource(R.string.details_open_extensions),
                            onAction = actions.onOpenExtensions,
                        )
                    } else {
                        SourceRow(
                            source = shown,
                            onClick = { actions.onOpenPicker(false) },
                            compact = true,
                            onDownload = if (shown.candidate.kind == SourceKind.TORRENT) {
                                { actions.onDownloadSource(shown, panel.episode) }
                            } else {
                                null
                            },
                        )
                        Row(
                            Modifier.padding(vertical = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                            horizontalArrangement = Arrangement.spacedBy(6.dp),
                        ) {
                            Icon(Icons.Filled.Check, contentDescription = null, modifier = Modifier.size(16.dp), tint = MaterialTheme.colorScheme.primary)
                            Text(
                                stringResource(if (state.selectedKey == null) R.string.details_src_by_system else R.string.details_src_by_you),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                        }
                        FilledTonalButton(onClick = { actions.onOpenPicker(false) }, modifier = Modifier.fillMaxWidth()) {
                            Text(stringResource(R.string.details_src_all, sources.value.size), modifier = Modifier.weight(1f))
                            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = null)
                        }
                    }
                }
            }
        }
    }
}

/** Side panel "Enviar a dispositivo" of the wide layout (prototype `details__side`). */
@Composable
internal fun SendToDevicePanel(state: DetailsUiState, actions: DetailsActions, modifier: Modifier = Modifier) {
    SectionCard(modifier = modifier, title = stringResource(R.string.details_send_device)) {
        val pc = state.pairedPc
        if (pc == null) {
            Text(
                stringResource(R.string.details_send_unpaired),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            TextButton(onClick = actions.onOpenLinked) { Text(stringResource(R.string.details_send_pair)) }
        } else {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Icon(Icons.Outlined.Computer, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                Column(Modifier.weight(1f)) {
                    Text(pc.serverName, style = MaterialTheme.typography.titleSmall)
                    Text(
                        stringResource(if (state.pcAvailable) R.string.details_send_online else R.string.details_send_offline),
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                Button(onClick = actions.onSendToDevice, enabled = !state.sending) {
                    Text(stringResource(R.string.details_send_action))
                }
            }
        }
    }
}

// ---------------------------------------------------------------- tabs

@Composable
private fun DetailsTabs(tab: DetailsTab, onTab: (DetailsTab) -> Unit) {
    PrimaryTabRow(selectedTabIndex = tab.ordinal, modifier = Modifier.padding(top = 8.dp)) {
        DetailsTab.entries.forEach { t ->
            Tab(
                selected = t == tab,
                onClick = { onTab(t) },
                text = { Text(stringResource(t.labelRes())) },
            )
        }
    }
}

private fun DetailsTab.labelRes(): Int = when (this) {
    DetailsTab.EPISODES -> R.string.details_episodes
    DetailsTab.INFO -> R.string.details_info
    DetailsTab.RELATED -> R.string.details_related
}

private fun LazyListScope.episodeItems(state: DetailsUiState, actions: DetailsActions, range: Int, onRange: (Int) -> Unit) {
    when (val eps = state.episodes) {
        LoadState.Loading -> item(key = "episodes-loading", contentType = "loading") { DelayedLoading() }
        is LoadState.Err -> item(key = "episodes-error", contentType = "error") {
            ErrorState(
                message = eps.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = actions.onRetry,
                compact = true,
            )
        }
        is LoadState.Ok -> {
            val all = eps.value
            if (all.isEmpty()) {
                item(key = "episodes-empty", contentType = "empty") {
                    EmptyState(
                        icon = Icons.Outlined.Movie,
                        title = stringResource(R.string.details_no_episodes_title),
                        message = stringResource(R.string.details_no_episodes),
                    )
                }
                return
            }
            val ranges = (all.size + RANGE_SIZE - 1) / RANGE_SIZE
            val selected = range.coerceIn(0, ranges - 1)
            if (ranges > 1) {
                item(key = "ranges", contentType = "ranges") {
                    Row(
                        Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        repeat(ranges) { i ->
                            val from = all[i * RANGE_SIZE].number
                            val to = all[minOf((i + 1) * RANGE_SIZE, all.size) - 1].number
                            FilterChip(
                                selected = i == selected,
                                onClick = { onRange(i) },
                                label = { Text(stringResource(R.string.details_range, from, to)) },
                            )
                        }
                    }
                }
            }
            val page = all.drop(selected * RANGE_SIZE).take(RANGE_SIZE)
            val current = state.continueEpisode
            items(page, key = { "ep-${it.number}" }, contentType = { "episode" }) { ep ->
                DetailsEpisodeRow(
                    episode = ep,
                    progress = state.progress[ep.number],
                    current = ep.number == current && state.progress.isNotEmpty(),
                    onClick = { actions.onEpisodeClick(ep.number) },
                    onDownload = { actions.onDownloadEpisode(ep.number) },
                )
            }
        }
    }
}

/** Prototype `.ep`: thumbnail with progress, "N. title", pills Relleno / Visto, air date, overflow menu with "Descargar episodio". */
@Composable
private fun DetailsEpisodeRow(
    episode: Episode,
    progress: HistoryEntry?,
    current: Boolean,
    onClick: () -> Unit,
    onDownload: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    val rowModifier = if (current) Modifier.background(MaterialTheme.colorScheme.surfaceContainer) else Modifier
    Row(
        Modifier
            .fillMaxWidth()
            .then(rowModifier)
            .clickable(onClick = onClick)
            .padding(start = 16.dp, end = 4.dp, top = 8.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.width(128.dp)) {
            MediaCover(url = episode.image, aspectRatio = 16f / 9f, modifier = Modifier.fillMaxWidth())
            val fraction = progress?.takeIf { !it.completed }?.progressFraction()
            if (fraction != null) {
                LinearProgressIndicator(
                    progress = { fraction },
                    modifier = Modifier.fillMaxWidth().align(Alignment.BottomCenter).padding(6.dp),
                )
            }
        }
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            val title = episode.title?.takeIf { it.isNotBlank() }
            Text(
                if (title != null) {
                    stringResource(R.string.details_episode_title, episode.number, title)
                } else {
                    stringResource(UiR.string.ui_episode_number, episode.number)
                },
                style = MaterialTheme.typography.titleSmall,
                color = if (current) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            val watched = progress?.completed == true
            if (episode.isFiller || watched) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (episode.isFiller) Pill(stringResource(R.string.details_filler), tone = PillTone.WARN)
                    if (watched) Pill(stringResource(R.string.details_watched), tone = PillTone.OK)
                }
            }
            episode.airDate?.let {
                Text(
                    stringResource(R.string.details_aired, it),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Box {
            IconButton(onClick = { menu = true }) {
                Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.details_episode_menu, episode.number))
            }
            DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.details_download_episode, episode.number)) },
                    onClick = {
                        menu = false
                        onDownload()
                    },
                    leadingIcon = { Icon(Icons.Outlined.Download, contentDescription = null) },
                )
            }
        }
    }
}

@Composable
private fun InfoTab(media: Media) {
    Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        SectionCard(title = stringResource(R.string.details_synopsis)) {
            Text(
                media.synopsis?.takeIf { it.isNotBlank() } ?: stringResource(R.string.details_no_synopsis),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        SectionCard(title = stringResource(R.string.details_info_ids)) {
            media.ref.anilistId?.let { KeyValue(stringResource(R.string.details_info_anilist), "#$it") }
            media.ref.malId?.let { KeyValue(stringResource(R.string.details_info_mal), "#$it") }
            media.ref.kitsuId?.let { KeyValue(stringResource(R.string.details_info_kitsu), "#$it") }
            KeyValue(stringResource(R.string.details_info_provider), media.sourceProvider)
            statusPill(media.status)?.let { (label, _) -> KeyValue(stringResource(R.string.details_info_status), label, mono = false) }
            val season = listOfNotNull(media.season?.let { seasonLabel(it) }, media.year?.toString()).joinToString(" ")
            if (season.isNotEmpty()) KeyValue(stringResource(R.string.details_info_season), season, mono = false)
            media.episodes?.let { KeyValue(stringResource(R.string.details_info_episodes), it.toString()) }
        }
        val others = media.titles.values.filter { it != media.title }.distinct()
        if (others.isNotEmpty()) {
            SectionCard(title = stringResource(R.string.details_info_titles)) {
                others.forEach { Text(it, style = MaterialTheme.typography.bodyMedium) }
            }
        }
    }
}

@Composable
private fun KeyValue(key: String, value: String, mono: Boolean = true) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(key, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium, fontFamily = if (mono) FontFamily.Monospace else null)
    }
}

/**
 * Related titles (same genre, see DetailsViewModel.loadRelated). DetailsScreen has no "open another title" callback in the contract,
 * so a card opens Search filtered by the genre it shares with this title.
 */
@Composable
private fun RelatedTab(media: Media, related: LoadState<List<Media>>?, actions: DetailsActions) {
    LaunchedEffect(Unit) { actions.onLoadRelated() }
    when (related) {
        null, LoadState.Loading -> DelayedLoading()
        is LoadState.Err -> ErrorState(
            message = related.cause.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = actions.onLoadRelated,
            compact = true,
        )
        is LoadState.Ok -> if (related.value.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.TravelExplore,
                title = stringResource(R.string.details_related_empty_title),
                message = stringResource(R.string.details_related_empty_body),
            )
        } else {
            Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                related.value.chunked(RELATED_COLUMNS).forEach { rowItems ->
                    Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                        rowItems.forEach { item ->
                            val shared = item.genres.firstOrNull { it in media.genres }
                            MediaCard(
                                media = item,
                                onClick = { shared?.let(actions.onOpenGenre) },
                                modifier = Modifier.weight(1f),
                            )
                        }
                        repeat(RELATED_COLUMNS - rowItems.size) { Spacer(Modifier.weight(1f)) }
                    }
                }
            }
        }
    }
}

@Composable
private fun DelayedLoading() {
    Box(Modifier.fillMaxWidth().height(120.dp)) {
        if (rememberAfterDelay()) LoadingState()
    }
}

// ---------------------------------------------------------------- labels

@Composable
private fun alternativeTitle(media: Media): String? =
    media.titles["romaji"]?.takeIf { it != media.title } ?: media.titles["native"]?.takeIf { it != media.title }

@Composable
internal fun statusPill(status: String?): Pair<String, PillTone>? = when (status?.uppercase()) {
    "RELEASING" -> stringResource(R.string.details_status_releasing) to PillTone.OK
    "FINISHED" -> stringResource(R.string.details_status_finished) to PillTone.NEUTRAL
    "NOT_YET_RELEASED" -> stringResource(R.string.details_status_upcoming) to PillTone.WARN
    "CANCELLED" -> stringResource(R.string.details_status_cancelled) to PillTone.ERR
    "HIATUS" -> stringResource(R.string.details_status_hiatus) to PillTone.WARN
    else -> null
}

@Composable
internal fun seasonLabel(season: String): String = when (season.uppercase()) {
    "WINTER" -> stringResource(R.string.details_season_winter)
    "SPRING" -> stringResource(R.string.details_season_spring)
    "SUMMER" -> stringResource(R.string.details_season_summer)
    "FALL" -> stringResource(R.string.details_season_fall)
    else -> season
}

private fun formatScore(score: Int): String = String.format(Locale.ROOT, "%.1f", score / 10.0)

private const val SYNOPSIS_LINES = 4
private const val SYNOPSIS_TOGGLE_CHARS = 120
private const val RELATED_COLUMNS = 3
