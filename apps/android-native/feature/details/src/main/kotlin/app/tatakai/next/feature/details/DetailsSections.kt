@file:OptIn(ExperimentalMaterial3ExpressiveApi::class, ExperimentalLayoutApi::class)

package app.tatakai.next.feature.details

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.automirrored.outlined.ViewList
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.Business
import androidx.compose.material.icons.outlined.Cast
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Extension
import androidx.compose.material.icons.outlined.GridView
import androidx.compose.material.icons.outlined.Groups
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material.icons.automirrored.outlined.OpenInNew
import androidx.compose.material.icons.outlined.PlayCircle
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.Search
import androidx.compose.material.icons.outlined.Share
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
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryScrollableTabRow
import androidx.compose.material3.SplitButtonDefaults
import androidx.compose.material3.SplitButtonLayout
import androidx.compose.material3.Switch
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.SectionCard
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.feature.downloads.DownloadSeasonSlot
import app.tatakai.next.feature.home.FollowShowRow
import app.tatakai.next.feature.library.ListButton
import app.tatakai.next.feature.social.AniListSocialSlot
import app.tatakai.next.feature.player.sources.SourceRow
import app.tatakai.next.model.CharacterEdge
import app.tatakai.next.model.Episode
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaDetails
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.MediaTag
import app.tatakai.next.model.NextAiring
import app.tatakai.next.model.Recommendation
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.model.SourceKind
import app.tatakai.next.model.SourceMode
import app.tatakai.next.model.StaffEdge
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.format.formatDuration
import app.tatakai.next.ui.media.MediaCard
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.alternativeTitle
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.media.progressFraction
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import java.util.Locale
import kotlinx.coroutines.delay

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
        DetailsTab.EPISODES -> {
            episodeItems(media, state, actions, range, onRange)
            item(key = "wave-b-episodes", contentType = "slot") { WaveBSlots(media) }
        }
        DetailsTab.INFO -> item(key = "info", contentType = "info") { InfoTab(media, state, actions) }
        DetailsTab.CHARACTERS -> charactersItems(state, actions)
        DetailsTab.RELATED -> item(key = "related", contentType = "related") { RelatedTab(media, state, actions) }
    }
}

/**
 * Mount points of wave B in Details (checklist `details.download-season-mount`, `details.comments-mount`): called at the end of the
 * Info tab and under the episode list. Nothing renders until wave B fills them.
 */
@Composable
internal fun WaveBSlots(media: Media) {
    // parity wave B (16 §3.1 / §3.2 / §3.7): the three slots are owned by r (FollowShowRow, DownloadSeasonSlot) and t (AniListSocialSlot);
    // each draws nothing until its owner implements it. This file stays closed: nobody else edits it.
    FollowShowRow(media)
    DownloadSeasonSlot(media)
    AniListSocialSlot(media)
}

/** Epoch seconds, refreshed every minute while composed (next-episode countdown). */
@Composable
private fun rememberNowSec(): Long {
    var now by remember { mutableLongStateOf(System.currentTimeMillis() / 1000) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(COUNTDOWN_TICK_MS)
            now = System.currentTimeMillis() / 1000
        }
    }
    return now
}

// ---------------------------------------------------------------- header

@Composable
private fun Header(media: Media, state: DetailsUiState, actions: DetailsActions) {
    var expanded by rememberSaveable { mutableStateOf(false) }
    val background = MaterialTheme.colorScheme.background
    val details = state.detailsOrNull
    val title = media.displayTitle(state.titleLanguage)
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
                Text(title, style = MaterialTheme.typography.headlineSmall, maxLines = 3, overflow = TextOverflow.Ellipsis)
                media.alternativeTitle(title)?.let {
                    Text(it, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 2)
                }
            }
        }
        MetadataChips(media, details, actions)
        details?.nextAiring?.let { NextEpisodeLine(it) }
        FlowRow(
            Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            WatchSplitButton(state, actions)
            ListButton(media = media)
            OutlinedButton(onClick = actions.onSendToDevice, enabled = !state.sending) {
                Icon(Icons.Outlined.Cast, contentDescription = null, modifier = Modifier.size(18.dp))
                Text(stringResource(R.string.details_send_device), modifier = Modifier.padding(start = 6.dp))
            }
            DownloadMenuButton(state, actions, icon = Icons.Outlined.Download, outlined = true)
            details?.trailer?.let { trailer ->
                OutlinedIconButton(onClick = { actions.onOpenUrl(trailer.url) }) {
                    Icon(Icons.Outlined.PlayCircle, contentDescription = stringResource(R.string.details_trailer))
                }
            }
            if (shareLink(media) != null) {
                OutlinedIconButton(onClick = { actions.onShare(media) }) {
                    Icon(Icons.Outlined.Share, contentDescription = stringResource(R.string.details_share))
                }
            }
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

/** "Episodio N en Xd Yh" (classic NextEpisodeSchedule), refreshed every minute; hidden once it aired. */
@Composable
private fun NextEpisodeLine(next: NextAiring) {
    val now = rememberNowSec()
    val left = countdown(next.airingAtEpochSec, now) ?: return
    val text = when {
        left.days > 0 -> stringResource(R.string.details_next_episode_days, next.episode, left.days, left.hours)
        left.hours > 0 -> stringResource(R.string.details_next_episode_hours, next.episode, left.hours, left.minutes)
        else -> stringResource(R.string.details_next_episode_minutes, next.episode, maxOf(1L, left.minutes))
    }
    Row(
        Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Icon(Icons.Outlined.Schedule, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
        Text(text, style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.primary)
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
private fun MetadataChips(media: Media, details: MediaDetails?, actions: DetailsActions) {
    val resources = LocalContext.current.resources
    FlowRow(
        Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        statusPill(media.status)?.let { (label, tone) -> Pill(label, tone = tone) }
        if (details?.isAdult == true || media.looksAdult()) Pill(stringResource(R.string.details_adult_badge), tone = PillTone.ERR)
        details?.format?.let { InfoChip(formatLabel(it)) }
        val season = listOfNotNull(media.year?.toString(), media.season?.let { seasonLabel(it) }).joinToString(" · ")
        if (season.isNotEmpty()) InfoChip(season)
        media.episodes?.let { InfoChip(resources.getQuantityString(R.plurals.details_episodes_count, it, it)) }
        media.score?.let { InfoChip(stringResource(R.string.details_score, formatScore(it)), icon = Icons.Filled.Star) }
        details?.studios?.forEach { studio ->
            AssistChip(
                onClick = { actions.onOpenStudio(studio) },
                label = { Text(studio) },
                leadingIcon = { Icon(Icons.Outlined.Business, contentDescription = null, modifier = Modifier.size(18.dp)) },
            )
        }
        media.genres.forEach { genre ->
            AssistChip(onClick = { actions.onOpenGenre(genre) }, label = { Text(genre) })
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
    PrimaryScrollableTabRow(selectedTabIndex = tab.ordinal, modifier = Modifier.padding(top = 8.dp)) {
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
    DetailsTab.CHARACTERS -> R.string.details_characters
    DetailsTab.RELATED -> R.string.details_related
}

/** Episode search field + list / compact toggle (classic AnimePage episode toolbar). */
@Composable
private fun EpisodeToolbar(state: DetailsUiState, actions: DetailsActions) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        OutlinedTextField(
            value = state.episodeQuery,
            onValueChange = actions.onEpisodeQuery,
            singleLine = true,
            leadingIcon = { Icon(Icons.Outlined.Search, contentDescription = null) },
            trailingIcon = if (state.episodeQuery.isNotEmpty()) {
                {
                    IconButton(onClick = { actions.onEpisodeQuery("") }) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.details_episode_search_clear))
                    }
                }
            } else {
                null
            },
            placeholder = { Text(stringResource(R.string.details_episode_search)) },
            modifier = Modifier.weight(1f),
        )
        Segmented(
            options = listOf(
                SegmentedOption(EpisodeView.LIST, stringResource(R.string.details_view_list), icon = Icons.AutoMirrored.Outlined.ViewList),
                SegmentedOption(EpisodeView.COMPACT, stringResource(R.string.details_view_compact), icon = Icons.Outlined.GridView),
            ),
            selected = state.episodeView,
            onSelect = actions.onEpisodeView,
            small = true,
        )
    }
}

private fun LazyListScope.episodeItems(media: Media, state: DetailsUiState, actions: DetailsActions, range: Int, onRange: (Int) -> Unit) {
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
            val nowSec = System.currentTimeMillis() / 1000
            val all = buildEpisodeItems(eps.value, state.detailsOrNull?.nextAiring, media.episodes, nowSec)
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
            item(key = "episode-toolbar", contentType = "toolbar") { EpisodeToolbar(state, actions) }
            val searching = state.episodeQuery.isNotBlank()
            val filtered = filterEpisodes(all, state.episodeQuery)
            if (filtered.isEmpty()) {
                item(key = "episodes-no-match", contentType = "empty") {
                    EmptyState(
                        icon = Icons.Outlined.Search,
                        title = stringResource(R.string.details_episode_search_empty_title),
                        message = stringResource(R.string.details_episode_search_empty, state.episodeQuery.trim()),
                    )
                }
                return
            }
            // While searching, every match shows (no ranges).
            val ranges = if (searching) 1 else (filtered.size + RANGE_SIZE - 1) / RANGE_SIZE
            val selected = range.coerceIn(0, ranges - 1)
            if (ranges > 1) {
                item(key = "ranges", contentType = "ranges") {
                    Row(
                        Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp, vertical = 8.dp),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        repeat(ranges) { i ->
                            val from = filtered[i * RANGE_SIZE].episode.number
                            val to = filtered[minOf((i + 1) * RANGE_SIZE, filtered.size) - 1].episode.number
                            FilterChip(
                                selected = i == selected,
                                onClick = { onRange(i) },
                                label = { Text(stringResource(R.string.details_range, from, to)) },
                            )
                        }
                    }
                }
            }
            val page = if (searching) filtered else filtered.drop(selected * RANGE_SIZE).take(RANGE_SIZE)
            val current = state.continueEpisode
            if (state.episodeView == EpisodeView.COMPACT) {
                page.chunked(COMPACT_COLUMNS).forEachIndexed { rowIndex, rowItems ->
                    item(key = "epc-$selected-$rowIndex", contentType = "episode-compact") {
                        Row(
                            Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            rowItems.forEach { item ->
                                CompactEpisodeCell(
                                    item = item,
                                    progress = state.progress[item.episode.number],
                                    current = item.episode.number == current && state.progress.isNotEmpty(),
                                    onClick = { actions.onEpisodeClick(item.episode.number) },
                                    modifier = Modifier.weight(1f),
                                )
                            }
                            repeat(COMPACT_COLUMNS - rowItems.size) { Spacer(Modifier.weight(1f)) }
                        }
                    }
                }
            } else {
                items(page, key = { "ep-${it.episode.number}" }, contentType = { "episode" }) { item ->
                    DetailsEpisodeRow(
                        episode = item.episode,
                        unaired = item.unaired,
                        progress = state.progress[item.episode.number],
                        current = item.episode.number == current && state.progress.isNotEmpty(),
                        downloaded = item.episode.number in state.downloaded,
                        onClick = { actions.onEpisodeClick(item.episode.number) },
                        onDownload = { actions.onDownloadEpisode(item.episode.number) },
                    )
                }
            }
        }
    }
}

/** Compact mode: a numbered cell; watched = tonal, current = outlined in primary, unaired = dimmed and not clickable. */
@Composable
private fun CompactEpisodeCell(
    item: EpisodeItem,
    progress: HistoryEntry?,
    current: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val watched = progress?.completed == true
    val container = when {
        watched -> MaterialTheme.colorScheme.secondaryContainer
        item.episode.isFiller -> MaterialTheme.colorScheme.tertiaryContainer
        else -> MaterialTheme.colorScheme.surfaceContainerHigh
    }
    val description = stringResource(
        if (item.unaired) R.string.details_episode_unaired_cd else UiR.string.ui_episode_number,
        item.episode.number,
    )
    Box(
        modifier
            .aspectRatio(1.4f)
            .clip(MaterialTheme.shapes.small)
            .background(container)
            .then(if (current) Modifier.border(2.dp, MaterialTheme.colorScheme.primary, MaterialTheme.shapes.small) else Modifier)
            .then(if (item.unaired) Modifier.alpha(UNAIRED_ALPHA) else Modifier.clickable(onClick = onClick))
            .padding(4.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            item.episode.number.toString(),
            style = MaterialTheme.typography.labelLarge,
            textAlign = TextAlign.Center,
            color = if (current) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.semantics { contentDescription = description },
        )
    }
}

/** Prototype `.ep`: thumbnail with progress, "N. title", pills Relleno / Visto, air date, overflow menu with "Descargar episodio". */
@Composable
private fun DetailsEpisodeRow(
    episode: Episode,
    unaired: Boolean,
    progress: HistoryEntry?,
    current: Boolean,
    onClick: () -> Unit,
    onDownload: () -> Unit,
    /** parity wave B (r): a completed download exists (plays offline). */
    downloaded: Boolean = false,
) {
    var menu by remember { mutableStateOf(false) }
    val rowModifier = if (current) Modifier.background(MaterialTheme.colorScheme.surfaceContainer) else Modifier
    Row(
        Modifier
            .fillMaxWidth()
            .then(rowModifier)
            .then(if (unaired) Modifier.alpha(UNAIRED_ALPHA) else Modifier.clickable(onClick = onClick))
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
            if (episode.isFiller || watched || unaired || downloaded) {
                Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    if (unaired) Pill(stringResource(R.string.details_unaired), tone = PillTone.NEUTRAL)
                    if (episode.isFiller) Pill(stringResource(R.string.details_filler), tone = PillTone.WARN)
                    if (watched) Pill(stringResource(R.string.details_watched), tone = PillTone.OK)
                    if (downloaded) Pill(stringResource(UiR.string.download_state_completed), tone = PillTone.NEUTRAL)
                }
            }
            episode.airDate?.let {
                Text(
                    stringResource(R.string.details_aired, formatAirDate(it)),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        if (!unaired) Box {
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

// ---------------------------------------------------------------- info

/** Technical sheet (classic AnimeDetailLayout facts), tags with relevance, trailer, external links, ids and titles. */
@Composable
private fun InfoTab(media: Media, state: DetailsUiState, actions: DetailsActions) {
    Column(Modifier.fillMaxWidth().padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
        SectionCard(title = stringResource(R.string.details_synopsis)) {
            Text(
                media.synopsis?.takeIf { it.isNotBlank() } ?: stringResource(R.string.details_no_synopsis),
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        when (val d = state.details) {
            LoadState.Loading -> DelayedLoading()
            is LoadState.Err -> ErrorState(
                message = d.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = actions.onRetryDetails,
                compact = true,
            )
            is LoadState.Ok -> d.value?.let { details -> FactsCards(details, actions) }
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
        val others = (media.titles.values + state.detailsOrNull?.synonyms.orEmpty())
            .filter { it.isNotBlank() && it != media.displayTitle(state.titleLanguage) }
            .distinct()
        if (others.isNotEmpty()) {
            SectionCard(title = stringResource(R.string.details_info_titles)) {
                others.forEach { Text(it, style = MaterialTheme.typography.bodyMedium) }
            }
        }
        WaveBSlots(media)
    }
}

@Composable
private fun FactsCards(details: MediaDetails, actions: DetailsActions) {
    SectionCard(title = stringResource(R.string.details_facts)) {
        details.format?.let { KeyValue(stringResource(R.string.details_fact_format), formatLabel(it), mono = false) }
        details.source?.let { KeyValue(stringResource(R.string.details_fact_source), sourceLabel(it), mono = false) }
        details.durationMin?.let { KeyValue(stringResource(R.string.details_fact_duration), stringResource(R.string.details_fact_minutes, it), mono = false) }
        details.startDate?.let { KeyValue(stringResource(R.string.details_fact_start), formatAirDate(it), mono = false) }
        details.endDate?.let { KeyValue(stringResource(R.string.details_fact_end), formatAirDate(it), mono = false) }
        details.meanScore?.let { KeyValue(stringResource(R.string.details_fact_mean_score), "$it%") }
        details.popularity?.let { KeyValue(stringResource(R.string.details_fact_popularity), String.format(Locale.getDefault(), "%,d", it)) }
        details.favourites?.let { KeyValue(stringResource(R.string.details_fact_favourites), String.format(Locale.getDefault(), "%,d", it)) }
        details.countryOfOrigin?.let { KeyValue(stringResource(R.string.details_fact_country), countryLabel(it), mono = false) }
        if (details.producers.isNotEmpty()) {
            KeyValue(stringResource(R.string.details_fact_producers), details.producers.joinToString(", "), mono = false)
        }
        if (details.studios.isNotEmpty()) {
            Text(
                stringResource(R.string.details_fact_studios),
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(top = 4.dp),
            )
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                details.studios.forEach { studio ->
                    AssistChip(
                        onClick = { actions.onOpenStudio(studio) },
                        label = { Text(studio) },
                        leadingIcon = { Icon(Icons.Outlined.Business, contentDescription = null, modifier = Modifier.size(18.dp)) },
                    )
                }
            }
        }
    }
    if (details.tags.isNotEmpty()) TagsCard(details.tags)
    details.trailer?.let { trailer ->
        SectionCard(title = stringResource(R.string.details_trailer)) {
            Box(
                Modifier
                    .fillMaxWidth()
                    .aspectRatio(16f / 9f)
                    .clip(MaterialTheme.shapes.medium)
                    .background(MaterialTheme.colorScheme.surfaceContainerHighest)
                    .clickable { actions.onOpenUrl(trailer.url) },
                contentAlignment = Alignment.Center,
            ) {
                trailer.thumbnail?.let {
                    AsyncImage(model = it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
                }
                Icon(
                    Icons.Outlined.PlayCircle,
                    contentDescription = stringResource(R.string.details_trailer_play),
                    tint = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.size(56.dp),
                )
            }
        }
    }
    if (details.externalLinks.isNotEmpty()) {
        SectionCard(title = stringResource(R.string.details_links)) {
            details.externalLinks.forEach { (label, url) ->
                TextButton(onClick = { actions.onOpenUrl(url) }) {
                    Text(label, modifier = Modifier.weight(1f, fill = false))
                    Icon(Icons.AutoMirrored.Outlined.OpenInNew, contentDescription = null, modifier = Modifier.padding(start = 6.dp).size(16.dp))
                }
            }
        }
    }
}

/** Tags with their relevance %; spoiler tags hidden unless the switch is on (classic behaviour). */
@Composable
private fun TagsCard(tags: List<MediaTag>) {
    var spoilers by rememberSaveable { mutableStateOf(false) }
    var all by rememberSaveable { mutableStateOf(false) }
    val visible = tags.filter { spoilers || !it.isSpoiler }.sortedByDescending { it.rank ?: 0 }
    val hiddenSpoilers = tags.count { it.isSpoiler }
    SectionCard(title = stringResource(R.string.details_tags)) {
        if (hiddenSpoilers > 0) {
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    stringResource(R.string.details_tags_spoilers, hiddenSpoilers),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.weight(1f),
                )
                Switch(checked = spoilers, onCheckedChange = { spoilers = it })
            }
        }
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            (if (all) visible else visible.take(TAGS_COLLAPSED)).forEach { tag ->
                Pill(
                    tag.rank?.let { stringResource(R.string.details_tag_rank, tag.name, it) } ?: tag.name,
                    tone = if (tag.isSpoiler) PillTone.WARN else PillTone.NEUTRAL,
                )
            }
        }
        if (!all && visible.size > TAGS_COLLAPSED) {
            TextButton(onClick = { all = true }) { Text(stringResource(R.string.details_tags_all, visible.size)) }
        }
    }
}

@Composable
private fun KeyValue(key: String, value: String, mono: Boolean = true) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        Text(key, style = MaterialTheme.typography.bodyMedium, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium, fontFamily = if (mono) FontFamily.Monospace else null, textAlign = TextAlign.End)
    }
}

// ---------------------------------------------------------------- characters

/** Characters tab: AniList character edges (paged, infinite scroll) with the Japanese voice actor, then the staff. */
private fun LazyListScope.charactersItems(state: DetailsUiState, actions: DetailsActions) {
    item(key = "characters-start", contentType = "trigger") {
        LaunchedEffect(Unit) { if (state.characters == null) actions.onLoadCharacters() }
    }
    val c = state.characters
    when {
        c == null || (c.items.isEmpty() && c.loading) -> item(key = "characters-loading", contentType = "loading") { DelayedLoading() }
        c.items.isEmpty() && c.error != null -> item(key = "characters-error", contentType = "error") {
            ErrorState(
                message = c.error.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = actions.onLoadCharacters,
                compact = true,
            )
        }
        c.items.isEmpty() -> item(key = "characters-empty", contentType = "empty") {
            EmptyState(
                icon = Icons.Outlined.Groups,
                title = stringResource(R.string.details_characters_empty_title),
                message = stringResource(R.string.details_characters_empty),
            )
        }
        else -> {
            items(c.items, key = { "ch-${it.character.id}" }, contentType = { "character" }) { edge ->
                CharacterEdgeRow(edge, onClick = { actions.onOpenCharacter(edge.character.id) })
            }
            if (c.hasNext || c.error != null) {
                item(key = "characters-more-${c.page}", contentType = "load-more") {
                    if (c.error == null) LaunchedEffect(c.page) { actions.onLoadCharacters() }
                    Box(Modifier.fillMaxWidth().padding(8.dp), contentAlignment = Alignment.Center) {
                        if (c.loading) {
                            DelayedLoading()
                        } else {
                            OutlinedButton(onClick = actions.onLoadCharacters) { Text(stringResource(R.string.details_characters_more)) }
                        }
                    }
                }
            }
        }
    }
    item(key = "staff", contentType = "staff") { StaffSection(state.staff) }
}

@Composable
private fun CharacterEdgeRow(edge: CharacterEdge, onClick: () -> Unit) {
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MediaCover(url = edge.character.image, modifier = Modifier.width(48.dp), shape = CircleShape, aspectRatio = 1f)
        Column(Modifier.weight(1f)) {
            Text(edge.character.name, style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            edge.role?.let {
                Text(characterRoleLabel(it), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
        }
        preferredVoiceActor(edge)?.let { va ->
            Column(Modifier.weight(1f), horizontalAlignment = Alignment.End) {
                Text(va.name, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.End)
                va.language?.let {
                    Text(languageLabel(it), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
            MediaCover(url = va.image, modifier = Modifier.width(48.dp), shape = CircleShape, aspectRatio = 1f)
        }
    }
}

@Composable
private fun StaffSection(staff: LoadState<List<StaffEdge>>?) {
    when (staff) {
        null, LoadState.Loading -> Unit
        is LoadState.Err -> Unit // the characters list carries the error / retry; staff is secondary
        is LoadState.Ok -> if (staff.value.isNotEmpty()) {
            SectionCard(modifier = Modifier.padding(16.dp), title = stringResource(R.string.details_staff)) {
                staff.value.forEach { edge ->
                    Row(
                        Modifier.fillMaxWidth().padding(vertical = 4.dp),
                        horizontalArrangement = Arrangement.spacedBy(12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        MediaCover(url = edge.staff.image, modifier = Modifier.width(40.dp), shape = CircleShape, aspectRatio = 1f)
                        Text(edge.staff.name, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                        edge.role?.let {
                            Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, textAlign = TextAlign.End)
                        }
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- related

/** Seasons / watch order (current highlighted), AniList relations by type and recommendations; every card opens its Details. */
@Composable
private fun RelatedTab(media: Media, state: DetailsUiState, actions: DetailsActions) {
    LaunchedEffect(Unit) { actions.onLoadRelated() }
    Column(Modifier.fillMaxWidth().padding(vertical = 16.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
        SeasonsSection(media, state.seasons, state.titleLanguage, actions)
        RelationsSection(state.relations, state.titleLanguage, actions)
        RecommendationsSection(state.recommendations, state.titleLanguage, actions)
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 16.dp))
}

@Composable
private fun SeasonsSection(media: Media, seasons: LoadState<List<Media>>?, language: TitleLanguage, actions: DetailsActions) {
    when (seasons) {
        null, LoadState.Loading -> DelayedLoading()
        is LoadState.Err -> ErrorState(
            message = seasons.cause.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = actions.onLoadRelated,
            compact = true,
            title = stringResource(R.string.details_seasons),
        )
        is LoadState.Ok -> if (seasons.value.size > 1) {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                SectionTitle(stringResource(R.string.details_seasons))
                Row(
                    Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    seasons.value.forEachIndexed { index, season ->
                        val isCurrent = season.ref == media.ref
                        Column(
                            Modifier
                                .width(GRID_CARD_WIDTH)
                                .then(
                                    if (isCurrent) {
                                        Modifier.border(2.dp, MaterialTheme.colorScheme.primary, MaterialTheme.shapes.medium).padding(4.dp)
                                    } else {
                                        Modifier
                                    },
                                ),
                        ) {
                            Text(
                                stringResource(if (isCurrent) R.string.details_season_current else R.string.details_season_order, index + 1),
                                style = MaterialTheme.typography.labelMedium,
                                color = if (isCurrent) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                            )
                            MediaCard(
                                title = season.displayTitle(language),
                                coverUrl = season.cover,
                                onClick = { if (!isCurrent) actions.onOpenMedia(season) },
                                subtitle = season.year?.toString(),
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun RelationsSection(relations: LoadState<List<MediaRelation>>?, language: TitleLanguage, actions: DetailsActions) {
    when (relations) {
        null, LoadState.Loading -> DelayedLoading()
        is LoadState.Err -> ErrorState(
            message = relations.cause.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = actions.onLoadRelated,
            compact = true,
            title = stringResource(R.string.details_related),
        )
        is LoadState.Ok -> if (relations.value.isEmpty()) {
            EmptyState(
                icon = Icons.Outlined.TravelExplore,
                title = stringResource(R.string.details_related_empty_title),
                message = stringResource(R.string.details_related_empty_body),
            )
        } else {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                SectionTitle(stringResource(R.string.details_related))
                Row(
                    Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    relations.value.forEach { rel ->
                        Column(Modifier.width(GRID_CARD_WIDTH)) {
                            Text(
                                relationLabel(rel.relation),
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.primary,
                                maxLines = 1,
                            )
                            MediaCard(
                                title = rel.media.displayTitle(language),
                                coverUrl = rel.media.cover,
                                // Manga / novels open their Details too (AniList ids); reading them is wave B.
                                onClick = { actions.onOpenMedia(rel.media) },
                                subtitle = listOfNotNull(mediaKindLabel(rel.media), rel.media.year?.toString()).joinToString(" · "),
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun RecommendationsSection(recs: LoadState<List<Recommendation>>?, language: TitleLanguage, actions: DetailsActions) {
    when (recs) {
        null, LoadState.Loading -> DelayedLoading()
        is LoadState.Err -> ErrorState(
            message = recs.cause.userMessage(),
            retryLabel = stringResource(UiR.string.ui_retry),
            onRetry = actions.onLoadRelated,
            compact = true,
            title = stringResource(R.string.details_recommendations),
        )
        is LoadState.Ok -> if (recs.value.isNotEmpty()) {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                SectionTitle(stringResource(R.string.details_recommendations))
                Row(
                    Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 16.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                ) {
                    recs.value.forEach { rec ->
                        MediaCard(
                            title = rec.media.displayTitle(language),
                            coverUrl = rec.media.cover,
                            onClick = { actions.onOpenMedia(rec.media) },
                            subtitle = if (rec.rating > 0) stringResource(R.string.details_recommendation_votes, rec.rating) else rec.media.year?.toString(),
                            modifier = Modifier.width(GRID_CARD_WIDTH),
                        )
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
private const val COMPACT_COLUMNS = 6
private const val TAGS_COLLAPSED = 12

/** `1999-10-20T14:15:00Z` / `1999-10-20` -> the localized medium date ("20 oct 1999"); anything else is shown as it came. */
internal fun formatAirDate(raw: String, locale: Locale = Locale.getDefault()): String =
    runCatching {
        java.time.LocalDate.parse(raw.take(10))
            .format(java.time.format.DateTimeFormatter.ofLocalizedDate(java.time.format.FormatStyle.MEDIUM).withLocale(locale))
    }.getOrDefault(raw)

private const val COUNTDOWN_TICK_MS = 60_000L
private const val UNAIRED_ALPHA = 0.5f
private val GRID_CARD_WIDTH = 120.dp

@Composable
internal fun formatLabel(format: String): String = when (format.uppercase()) {
    "TV" -> stringResource(R.string.details_format_tv)
    "TV_SHORT" -> stringResource(R.string.details_format_tv_short)
    "MOVIE" -> stringResource(R.string.details_format_movie)
    "OVA" -> stringResource(R.string.details_format_ova)
    "ONA" -> stringResource(R.string.details_format_ona)
    "SPECIAL" -> stringResource(R.string.details_format_special)
    "MUSIC" -> stringResource(R.string.details_format_music)
    "MANGA" -> stringResource(R.string.details_format_manga)
    "NOVEL" -> stringResource(R.string.details_format_novel)
    "ONE_SHOT" -> stringResource(R.string.details_format_one_shot)
    else -> format
}

/** AniList `MediaSource`. */
@Composable
internal fun sourceLabel(source: String): String = when (source.uppercase()) {
    "ORIGINAL" -> stringResource(R.string.details_source_original)
    "MANGA" -> stringResource(R.string.details_source_manga)
    "LIGHT_NOVEL" -> stringResource(R.string.details_source_light_novel)
    "NOVEL" -> stringResource(R.string.details_source_novel)
    "WEB_NOVEL" -> stringResource(R.string.details_source_web_novel)
    "VISUAL_NOVEL" -> stringResource(R.string.details_source_visual_novel)
    "VIDEO_GAME", "GAME" -> stringResource(R.string.details_source_game)
    "WEB_MANGA" -> stringResource(R.string.details_source_web_manga)
    "ANIME" -> stringResource(R.string.details_source_anime)
    else -> stringResource(R.string.details_source_other)
}

/** AniList `MediaRelation`. */
@Composable
internal fun relationLabel(relation: String): String = when (relation.uppercase()) {
    "PREQUEL" -> stringResource(R.string.details_rel_prequel)
    "SEQUEL" -> stringResource(R.string.details_rel_sequel)
    "PARENT" -> stringResource(R.string.details_rel_parent)
    "SIDE_STORY" -> stringResource(R.string.details_rel_side_story)
    "SPIN_OFF" -> stringResource(R.string.details_rel_spin_off)
    "ALTERNATIVE" -> stringResource(R.string.details_rel_alternative)
    "SUMMARY" -> stringResource(R.string.details_rel_summary)
    "ADAPTATION" -> stringResource(R.string.details_rel_adaptation)
    "SOURCE" -> stringResource(R.string.details_rel_source)
    "CHARACTER" -> stringResource(R.string.details_rel_character)
    "COMPILATION" -> stringResource(R.string.details_rel_compilation)
    "CONTAINS" -> stringResource(R.string.details_rel_contains)
    else -> stringResource(R.string.details_rel_other)
}

@Composable
private fun mediaKindLabel(media: Media): String? =
    if (media.kind == app.tatakai.next.model.MediaKind.MANGA) stringResource(R.string.details_kind_manga) else null

/** ISO 3166 code of AniList `countryOfOrigin` -> localized country name. */
internal fun countryLabel(code: String, locale: Locale = Locale.getDefault()): String =
    Locale("", code).getDisplayCountry(locale).takeIf { it.isNotBlank() && it != code } ?: code
