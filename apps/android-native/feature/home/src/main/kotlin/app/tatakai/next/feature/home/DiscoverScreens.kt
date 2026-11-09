// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import android.Manifest
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.outlined.BookmarkAdd
import androidx.compose.material.icons.outlined.Bookmark
import androidx.compose.material.icons.outlined.Explore
import androidx.compose.material.icons.outlined.Hub
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material3.AssistChip
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.InfoChip
import app.tatakai.next.designsystem.components.ListGroupHeader
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.AiringRepository
import app.tatakai.next.model.AtlasPoint
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaKind
import app.tatakai.next.model.RecReason
import app.tatakai.next.model.RecReasonKind
import app.tatakai.next.model.RecommendationItem
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.LocalTitleLanguage
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.state.userMessage
import kotlinx.coroutines.launch
import org.koin.compose.getKoin
import org.koin.compose.viewmodel.koinViewModel
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import kotlin.math.hypot
import kotlin.math.roundToInt

// Parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.1, worker r). Signatures FINAL (the shell and Details
// wire them).

/** Asks POST_NOTIFICATIONS (API 33+) once the user turns on something that notifies (never at app start, 16 §0.12). */
@Composable
internal fun rememberNotificationPermission(): () -> Unit {
    val context = LocalContext.current
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    return remember(launcher) {
        {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
                ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED
            ) {
                launcher.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun DiscoverTopBar(title: String, onBack: () -> Unit, actions: @Composable () -> Unit = {}) {
    TopAppBar(
        title = { Text(title) },
        navigationIcon = {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) }
        },
        actions = { actions() },
    )
}

/**
 * `Route.Calendar` (checklist home.calendar.schedule / .my-shows; plan §7 D6: the calendar lives inside Inicio): week view grouped by
 * day (`AiringRepository.schedule`), local time, followed shows highlighted with "Dejar de seguir" / reminder toggle, "Solo las que sigo"
 * filter. Reached from the Home row header "Calendario".
 */
@Composable
fun CalendarScreen(onBack: () -> Unit, onMediaClick: (Media) -> Unit, modifier: Modifier = Modifier) {
    val vm: CalendarViewModel = koinViewModel()
    val state by vm.state.collectAsStateWithLifecycle()
    val askPermission = rememberNotificationPermission()
    val zone = remember { ZoneId.systemDefault() }
    val time = remember { DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT) }
    val dayFmt = remember { DateTimeFormatter.ofPattern("EEEE d MMM") }
    Scaffold(modifier = modifier, topBar = { DiscoverTopBar(stringResource(R.string.home_calendar_title), onBack) }) { padding ->
        LazyColumn(Modifier.fillMaxSize().padding(padding), contentPadding = PaddingValues(bottom = 24.dp)) {
            item(key = "week") {
                Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                    TextButton(onClick = { vm.shiftWeek(-1) }) { Text(stringResource(R.string.home_calendar_prev_week)) }
                    Text(
                        stringResource(R.string.home_calendar_week_of, state.weekStart.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM))),
                        style = MaterialTheme.typography.titleSmall,
                        modifier = Modifier.weight(1f),
                        maxLines = 1,
                    )
                    TextButton(onClick = { vm.shiftWeek(1) }) { Text(stringResource(R.string.home_calendar_next_week)) }
                }
            }
            item(key = "filter") {
                Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    FilterChip(
                        selected = state.onlyFollowed,
                        onClick = { vm.setOnlyFollowed(!state.onlyFollowed) },
                        label = { Text(stringResource(R.string.home_calendar_only_followed)) },
                    )
                }
            }
            if (state.followed.isNotEmpty()) {
                item(key = "followed-header") { ListGroupHeader(stringResource(R.string.home_calendar_my_shows)) }
                items(state.followed, key = { "f:" + (it.ref.anilistId ?: it.ref.malId) }) { show ->
                    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                        MediaCover(url = show.poster, modifier = Modifier.width(40.dp))
                        Text(show.title, Modifier.weight(1f).padding(horizontal = 12.dp), maxLines = 2, overflow = TextOverflow.Ellipsis)
                        IconToggleButton(checked = show.remind, onCheckedChange = { on ->
                            if (on) askPermission()
                            vm.setReminder(show.ref, on)
                        }) {
                            Icon(
                                if (show.remind) Icons.Outlined.NotificationsActive else Icons.Outlined.Notifications,
                                contentDescription = stringResource(R.string.home_follow_reminder),
                            )
                        }
                        TextButton(onClick = { vm.unfollow(show.ref) }) { Text(stringResource(R.string.home_unfollow)) }
                    }
                }
            }
            when (val days = state.visibleDays) {
                LoadState.Loading -> item(key = "loading") { if (rememberAfterDelay()) LoadingState(Modifier.padding(32.dp)) }
                is LoadState.Err -> item(key = "error") {
                    ErrorState(message = days.cause.userMessage(), retryLabel = stringResource(UiR.string.ui_retry), onRetry = vm::load)
                }
                is LoadState.Ok -> {
                    if (days.value.all { it.episodes.isEmpty() }) {
                        item(key = "empty") {
                            EmptyState(
                                icon = Icons.Outlined.Schedule,
                                title = stringResource(R.string.home_calendar_empty),
                                message = stringResource(if (state.onlyFollowed) R.string.home_calendar_empty_followed else R.string.home_calendar_empty_body),
                            )
                        }
                    }
                    days.value.filter { it.episodes.isNotEmpty() }.forEach { day ->
                        item(key = "d:${day.date}") { ListGroupHeader(day.date.format(dayFmt).replaceFirstChar { it.titlecase() }) }
                        items(day.episodes, key = { "e:${day.date}:${it.media.ref.anilistId}:${it.episode}" }) { ep ->
                            CalendarRow(
                                ep = ep,
                                time = Instant.ofEpochSecond(ep.airingAtEpochSec).atZone(zone).format(time),
                                tracked = state.isTracked(ep.media.ref),
                                followed = state.isFollowed(ep.media.ref),
                                onClick = { onMediaClick(ep.media) },
                                onToggleFollow = {
                                    if (!state.isFollowed(ep.media.ref)) askPermission()
                                    vm.toggleFollow(ep)
                                },
                            )
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun CalendarRow(ep: AiringEpisode, time: String, tracked: Boolean, followed: Boolean, onClick: () -> Unit, onToggleFollow: () -> Unit) {
    Surface(
        onClick = onClick,
        color = if (tracked) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surface,
        shape = MaterialTheme.shapes.medium,
        modifier = Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 3.dp),
    ) {
        Row(Modifier.padding(8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            MediaCover(url = ep.media.cover, modifier = Modifier.width(44.dp))
            Column(Modifier.weight(1f)) {
                Text(ep.media.displayTitle(LocalTitleLanguage.current), style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
                Text(
                    stringResource(R.string.home_calendar_episode_at, ep.episode, time),
                    style = MaterialTheme.typography.labelMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            IconButton(onClick = onToggleFollow) {
                Icon(
                    if (followed) Icons.Outlined.Bookmark else Icons.Outlined.BookmarkAdd,
                    contentDescription = stringResource(if (followed) R.string.home_unfollow else R.string.home_follow),
                )
            }
        }
    }
}

/**
 * `Route.ForYou` (checklist home.recs.for-you / .explanations / .feedback / .atlas): the full "Para ti" list (`RecsRepository.forYou`)
 * with affinity %, reasons chips, "No me interesa" (undo snackbar), the taste profile summary and the atlas tab (`atlas()`, Canvas with
 * clusters; tap a dot → [onMediaClick]).
 */
@Composable
fun ForYouScreen(onBack: () -> Unit, onMediaClick: (Media) -> Unit, modifier: Modifier = Modifier) {
    val vm: ForYouViewModel = koinViewModel()
    val state by vm.state.collectAsStateWithLifecycle()
    val snackbar = LocalAppSnackbar.current
    val undoLabel = stringResource(R.string.home_recs_undo)
    val dismissedMsg = stringResource(R.string.home_recs_dismissed)
    Scaffold(modifier = modifier, topBar = { DiscoverTopBar(stringResource(R.string.home_for_you_title), onBack) }) { padding ->
        Column(Modifier.fillMaxSize().padding(padding)) {
            Segmented(
                options = listOf(
                    SegmentedOption(ForYouTab.LIST, stringResource(R.string.home_recs_tab_list), icon = Icons.Outlined.Explore),
                    SegmentedOption(ForYouTab.ATLAS, stringResource(R.string.home_recs_tab_atlas), icon = Icons.Outlined.Hub),
                ),
                selected = state.tab,
                onSelect = vm::selectTab,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            )
            when (state.tab) {
                ForYouTab.LIST -> RecsList(state, onMediaClick, onRetry = vm::load, onNotInterested = { item ->
                    vm.notInterested(item.media.ref)
                    snackbar.show(dismissedMsg, undoLabel) { vm.undo(item.media.ref) }
                })
                ForYouTab.ATLAS -> AtlasTab(state.atlas, onMediaClick, onRetry = vm::loadAtlas)
            }
        }
    }
}

@Composable
private fun RecsList(state: ForYouUiState, onMediaClick: (Media) -> Unit, onRetry: () -> Unit, onNotInterested: (RecommendationItem) -> Unit) {
    when (val items = state.visible) {
        LoadState.Loading -> if (rememberAfterDelay()) LoadingState()
        is LoadState.Err -> ErrorState(message = items.cause.userMessage(), retryLabel = stringResource(UiR.string.ui_retry), onRetry = onRetry)
        is LoadState.Ok -> if (items.value.isEmpty()) {
            EmptyState(icon = Icons.Outlined.Explore, title = stringResource(R.string.home_recs_empty), message = stringResource(R.string.home_recs_empty_body))
        } else {
            LazyColumn(contentPadding = PaddingValues(bottom = 24.dp)) {
                state.profile?.let { p ->
                    item(key = "profile") {
                        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                            Text(stringResource(R.string.home_recs_profile, p.sampleSize), style = MaterialTheme.typography.titleSmall)
                            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                                p.genres.entries.sortedByDescending { it.value }.take(6).forEach { InfoChip(it.key) }
                            }
                        }
                    }
                }
                items(items.value, key = { "r:" + (it.media.ref.anilistId ?: it.media.ref.malId) }) { item ->
                    RecRow(item, onClick = { onMediaClick(item.media) }, onNotInterested = { onNotInterested(item) })
                }
            }
        }
    }
}

@Composable
internal fun reasonLabel(reason: RecReason): String = when (reason.kind) {
    RecReasonKind.GENRE -> stringResource(R.string.home_recs_reason_genre, reason.label)
    RecReasonKind.STUDIO -> stringResource(R.string.home_recs_reason_studio, reason.label)
    RecReasonKind.ERA -> stringResource(R.string.home_recs_reason_era, reason.label)
    RecReasonKind.SCORE -> stringResource(R.string.home_recs_reason_score)
    RecReasonKind.SIMILAR_TO -> stringResource(R.string.home_recs_reason_similar, reason.label)
}

@Composable
private fun RecRow(item: RecommendationItem, onClick: () -> Unit, onNotInterested: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        MediaCover(url = item.media.cover, modifier = Modifier.width(64.dp))
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(item.media.displayTitle(LocalTitleLanguage.current), style = MaterialTheme.typography.titleSmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
            Pill(stringResource(R.string.home_recs_affinity, (item.affinity * 100).roundToInt()), tone = PillTone.OK)
            Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                item.reasons.forEach { InfoChip(reasonLabel(it)) }
            }
            TextButton(onClick = onNotInterested, contentPadding = PaddingValues(horizontal = 0.dp)) { Text(stringResource(R.string.home_recs_not_interested)) }
        }
    }
}

@Composable
private fun AtlasTab(atlas: LoadState<List<AtlasPoint>>?, onMediaClick: (Media) -> Unit, onRetry: () -> Unit) {
    when (atlas) {
        null, LoadState.Loading -> if (rememberAfterDelay()) LoadingState()
        is LoadState.Err -> ErrorState(message = atlas.cause.userMessage(), retryLabel = stringResource(UiR.string.ui_retry), onRetry = onRetry)
        is LoadState.Ok -> if (atlas.value.isEmpty()) {
            EmptyState(icon = Icons.Outlined.Hub, title = stringResource(R.string.home_recs_atlas_empty), message = stringResource(R.string.home_recs_empty_body))
        } else {
            AtlasCanvas(atlas.value, onMediaClick)
        }
    }
}

@Composable
private fun AtlasCanvas(points: List<AtlasPoint>, onMediaClick: (Media) -> Unit) {
    val palette = listOf(
        MaterialTheme.colorScheme.primary, MaterialTheme.colorScheme.tertiary, MaterialTheme.colorScheme.secondary,
        MaterialTheme.colorScheme.error, MaterialTheme.colorScheme.inversePrimary, MaterialTheme.colorScheme.outline,
    )
    val ring = MaterialTheme.colorScheme.onSurface
    Column(Modifier.fillMaxSize().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.home_recs_atlas_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        Box(Modifier.fillMaxWidth().aspectRatio(1f)) {
            Canvas(
                Modifier.fillMaxSize().pointerInput(points) {
                    detectTapGestures { tap ->
                        val pad = 24f
                        val w = size.width - 2 * pad
                        val h = size.height - 2 * pad
                        val hit = points.minByOrNull { p -> hypot(pad + p.x * w - tap.x, pad + p.y * h - tap.y) }
                        if (hit != null && hypot(pad + hit.x * w - tap.x, pad + hit.y * h - tap.y) < 48f) onMediaClick(hit.media)
                    }
                },
            ) {
                val pad = 24f
                val w = size.width - 2 * pad
                val h = size.height - 2 * pad
                points.forEach { p ->
                    val c = Offset(pad + p.x * w, pad + p.y * h)
                    val color = palette[p.cluster.mod(palette.size)]
                    drawCircle(color = color, radius = if (p.recommended) 9f else 14f, center = c)
                    if (p.recommended) drawCircle(color = ring, radius = 12f, center = c, style = androidx.compose.ui.graphics.drawscope.Stroke(width = 2f))
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            InfoChip(stringResource(R.string.home_recs_atlas_seen))
            InfoChip(stringResource(R.string.home_recs_atlas_recommended))
        }
    }
}

/**
 * Checklist details.track-show: "Seguir" / "Siguiendo" row mounted by `WaveBSlots` of Details (o's file, closed) under the episode list;
 * toggles `AiringRepository.follow / unfollow` and the reminder switch (`setReminder`). Only for `kind == ANIME` and
 * `status == RELEASING | NOT_YET_RELEASED`.
 */
@Composable
fun FollowShowRow(media: Media, modifier: Modifier = Modifier) {
    if (media.kind != MediaKind.ANIME) return
    val status = media.status?.uppercase()
    if (status != "RELEASING" && status != "NOT_YET_RELEASED") return
    val koin = getKoin()
    val airing = remember { koin.getOrNull<AiringRepository>() } ?: return
    val followedFlow = remember(media.ref) { airing.isFollowed(media.ref) }
    val following by followedFlow.collectAsStateWithLifecycle(initialValue = false)
    val shows by airing.followed.collectAsStateWithLifecycle()
    val show = shows.firstOrNull { sameRef(it.ref, media.ref) }
    val scope = rememberCoroutineScope()
    val askPermission = rememberNotificationPermission()
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = MaterialTheme.shapes.large, modifier = modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Column(Modifier.weight(1f)) {
                    Text(stringResource(R.string.home_follow_title), style = MaterialTheme.typography.titleSmall)
                    Text(stringResource(R.string.home_follow_body), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (following) {
                    OutlinedButton(onClick = { scope.launch { runCatching { airing.unfollow(media.ref) } } }) {
                        Icon(Icons.Outlined.Bookmark, contentDescription = null)
                        Text(stringResource(R.string.home_following), Modifier.padding(start = 6.dp))
                    }
                } else {
                    FilledTonalButton(onClick = {
                        askPermission()
                        scope.launch { runCatching { airing.follow(media, remind = true) } }
                    }) {
                        Icon(Icons.Outlined.BookmarkAdd, contentDescription = null)
                        Text(stringResource(R.string.home_follow), Modifier.padding(start = 6.dp))
                    }
                }
            }
            if (following && show != null) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(Icons.Outlined.NotificationsActive, contentDescription = null)
                    Text(stringResource(R.string.home_follow_reminder), Modifier.weight(1f).padding(horizontal = 12.dp))
                    Switch(checked = show.remind, onCheckedChange = { on ->
                        if (on) askPermission()
                        scope.launch { runCatching { airing.setReminder(media.ref, on) } }
                    })
                }
            }
        }
    }
}

/** Kept small for the Home rows: a header with an arrow to a full screen. */
@Composable
internal fun SeeAllArrow(onClick: () -> Unit) {
    IconButton(onClick = onClick) { Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = stringResource(R.string.home_see_all)) }
}

/** Progress line for a download card of the "Descargando" row. */
@Composable
internal fun ProgressLine(progress: Float) {
    LinearProgressIndicator(progress = { progress.coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth())
}

/** Assist chip helper (genre cloud). */
@Composable
internal fun GenreChip(genre: String, onClick: () -> Unit) {
    AssistChip(onClick = onClick, label = { Text(genre) })
}
