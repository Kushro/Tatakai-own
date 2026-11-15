// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowLeft
import androidx.compose.material.icons.automirrored.filled.KeyboardArrowRight
import androidx.compose.material.icons.outlined.CalendarMonth
import androidx.compose.material.icons.outlined.CalendarViewDay
import androidx.compose.material.icons.outlined.CalendarViewWeek
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.Today
import androidx.compose.material.icons.outlined.ViewAgenda
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.ListGroupHeader
import app.tatakai.next.designsystem.components.LoadingState
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.CalendarView
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.ui.look.lookFadeSpec
import app.tatakai.next.ui.media.LocalTitleLanguage
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.state.userMessage
import java.time.Instant
import java.time.LocalDate
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.time.format.TextStyle
import java.util.Locale
import org.koin.compose.viewmodel.koinViewModel
import app.tatakai.next.ui.R as UiR

/**
 * `Route.Calendar` (checklist home.calendar.schedule / .my-shows): a bar / rail destination since the 2026-11 UI refresh, with the
 * four views of the desktop calendar (Diseño y estilo › Calendario is the default; the selector here changes it and remembers it):
 * - Semana: seven columns on wide windows; on phones a strip of the seven days and the selected day's episodes below.
 * - Agenda: one header per day with its episodes (the previous Android calendar).
 * - Mes: month grid with up to three posters (wide) or dots (phone) per day; tapping a day lists it below.
 * - Hoy: the day as a time line by hour with a "now" marker.
 * Followed / WATCHING titles are highlighted; "Solo las que sigo" filters every view.
 */
@Composable
fun CalendarScreen(onBack: (() -> Unit)?, onMediaClick: (Media) -> Unit, modifier: Modifier = Modifier) {
    val vm: CalendarViewModel = koinViewModel()
    val state by vm.state.collectAsStateWithLifecycle()
    val askPermission = rememberNotificationPermission()
    val zone = remember { ZoneId.systemDefault() }
    val ctx = CalendarCtx(
        state = state,
        zone = zone,
        time = remember { DateTimeFormatter.ofLocalizedTime(FormatStyle.SHORT) },
        onOpen = onMediaClick,
        onToggleFollow = { ep ->
            if (!state.isFollowed(ep.media.ref)) askPermission()
            vm.toggleFollow(ep)
        },
    )
    Scaffold(
        modifier = modifier,
        topBar = {
            DiscoverTopBar(stringResource(R.string.home_calendar_title), onBack) {
                IconButton(onClick = vm::goToday) { Icon(Icons.Outlined.Today, contentDescription = stringResource(R.string.home_cal_today)) }
            }
        },
    ) { padding ->
        BoxWithConstraints(Modifier.fillMaxSize().padding(padding)) {
            val wide = maxWidth >= 720.dp
            LazyColumn(Modifier.fillMaxSize(), contentPadding = PaddingValues(bottom = 24.dp)) {
                item(key = "views", contentType = "views") {
                    Segmented(
                        options = listOf(
                            SegmentedOption(CalendarView.WEEK, stringResource(R.string.home_cal_view_week), icon = Icons.Outlined.CalendarViewWeek),
                            SegmentedOption(CalendarView.AGENDA, stringResource(R.string.home_cal_view_agenda), icon = Icons.Outlined.ViewAgenda),
                            SegmentedOption(CalendarView.MONTH, stringResource(R.string.home_cal_view_month), icon = Icons.Outlined.CalendarMonth),
                            SegmentedOption(CalendarView.DAY, stringResource(R.string.home_cal_view_day), icon = Icons.Outlined.CalendarViewDay),
                        ),
                        selected = state.view,
                        onSelect = vm::setView,
                        fillWidth = true,
                        small = true,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                    )
                }
                item(key = "pager", contentType = "pager") {
                    PageHeader(state.view, state.anchor, onShift = vm::shift)
                }
                item(key = "filter", contentType = "filter") {
                    Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        FilterChip(
                            selected = state.onlyFollowed,
                            onClick = { vm.setOnlyFollowed(!state.onlyFollowed) },
                            label = { Text(stringResource(R.string.home_calendar_only_followed)) },
                        )
                    }
                }
                when (val days = state.visibleDays) {
                    LoadState.Loading -> item(key = "loading") { if (rememberAfterDelay()) LoadingState(Modifier.padding(32.dp)) }
                    is LoadState.Err -> item(key = "error") {
                        ErrorState(message = days.cause.userMessage(), retryLabel = stringResource(UiR.string.ui_retry), onRetry = vm::load)
                    }
                    is LoadState.Ok -> when (state.view) {
                        CalendarView.AGENDA -> agenda(days.value, ctx)
                        CalendarView.WEEK -> if (wide) {
                            item(key = "week-grid", contentType = "week-grid") { WeekColumns(days.value, ctx) }
                        } else {
                            item(key = "week-strip", contentType = "week-strip") { WeekStrip(days.value, state.anchor, vm::selectDay) }
                            selectedDay(days.value, state.anchor, ctx)
                        }
                        CalendarView.MONTH -> {
                            item(key = "month-grid", contentType = "month-grid") { MonthGrid(days.value, state.anchor, wide, ctx, vm::selectDay) }
                            selectedDay(days.value, state.anchor, ctx)
                        }
                        CalendarView.DAY -> dayTimeline(days.value.firstOrNull()?.episodes.orEmpty(), state.anchor, ctx)
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
            }
        }
    }
}

/** What every view needs to draw an episode. */
private class CalendarCtx(
    val state: CalendarUiState,
    val zone: ZoneId,
    val time: DateTimeFormatter,
    val onOpen: (Media) -> Unit,
    val onToggleFollow: (AiringEpisode) -> Unit,
) {
    fun timeOf(ep: AiringEpisode): String = Instant.ofEpochSecond(ep.airingAtEpochSec).atZone(zone).format(time)
}

@Composable
private fun PageHeader(view: CalendarView, anchor: LocalDate, onShift: (Long) -> Unit) {
    val title = when (view) {
        CalendarView.WEEK, CalendarView.AGENDA -> stringResource(
            R.string.home_calendar_week_of,
            CalendarLogic.mondayOf(anchor).format(DateTimeFormatter.ofLocalizedDate(FormatStyle.MEDIUM)),
        )
        CalendarView.MONTH -> anchor.format(DateTimeFormatter.ofPattern("LLLL yyyy")).replaceFirstChar { it.titlecase() }
        CalendarView.DAY -> anchor.format(DateTimeFormatter.ofLocalizedDate(FormatStyle.FULL)).replaceFirstChar { it.titlecase() }
    }
    val fade = lookFadeSpec<Float>()
    Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        IconButton(onClick = { onShift(-1) }) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowLeft, contentDescription = stringResource(R.string.home_calendar_prev_week))
        }
        AnimatedContent(
            targetState = title,
            transitionSpec = { fadeIn(fade) togetherWith fadeOut(fade) },
            modifier = Modifier.weight(1f),
            label = "calendarTitle",
        ) { t ->
            Text(t, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
        }
        IconButton(onClick = { onShift(1) }) {
            Icon(Icons.AutoMirrored.Filled.KeyboardArrowRight, contentDescription = stringResource(R.string.home_calendar_next_week))
        }
    }
}

private fun dayTitle(date: LocalDate): String =
    date.format(DateTimeFormatter.ofPattern("EEEE d MMM")).replaceFirstChar { it.titlecase() }

private fun emptyItem(scope: LazyListScope, key: String, onlyFollowed: Boolean, whole: Boolean) {
    scope.item(key = key) {
        EmptyState(
            icon = Icons.Outlined.Schedule,
            title = stringResource(if (whole) R.string.home_calendar_empty else R.string.home_cal_empty_day),
            message = stringResource(if (onlyFollowed) R.string.home_calendar_empty_followed else R.string.home_calendar_empty_body),
        )
    }
}

/** Agenda: one header per day that has episodes. */
private fun LazyListScope.agenda(days: List<CalendarDay>, ctx: CalendarCtx) {
    if (days.all { it.episodes.isEmpty() }) {
        emptyItem(this, "empty", ctx.state.onlyFollowed, whole = true)
        return
    }
    days.filter { it.episodes.isNotEmpty() }.forEach { day ->
        item(key = "d:${day.date}", contentType = "day-header") { ListGroupHeader(dayTitle(day.date)) }
        episodeItems(day.date.toString(), day.episodes, ctx)
    }
}

private fun LazyListScope.episodeItems(prefix: String, episodes: List<AiringEpisode>, ctx: CalendarCtx) {
    items(episodes, key = { "e:$prefix:${it.media.ref.anilistId ?: it.media.ref.malId}:${it.episode}" }, contentType = { "episode" }) { ep ->
        CalendarRow(
            ep = ep,
            time = ctx.timeOf(ep),
            tracked = ctx.state.isTracked(ep.media.ref),
            followed = ctx.state.isFollowed(ep.media.ref),
            onClick = { ctx.onOpen(ep.media) },
            onToggleFollow = { ctx.onToggleFollow(ep) },
        )
    }
}

/** The selected day of the week strip / month grid. */
private fun LazyListScope.selectedDay(days: List<CalendarDay>, anchor: LocalDate, ctx: CalendarCtx) {
    val day = days.firstOrNull { it.date == anchor } ?: return
    item(key = "sel:${day.date}", contentType = "day-header") { ListGroupHeader(dayTitle(day.date)) }
    if (day.episodes.isEmpty()) emptyItem(this, "sel-empty", ctx.state.onlyFollowed, whole = false)
    else episodeItems("sel", day.episodes, ctx)
}

/** Phone week: seven touch targets (weekday, day number, episode count), the selected one filled. */
@Composable
private fun WeekStrip(days: List<CalendarDay>, selected: LocalDate, onSelect: (LocalDate) -> Unit) {
    val today = LocalDate.now()
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        days.forEach { day ->
            val isSel = day.date == selected
            val count = day.episodes.size
            val description = dayTitle(day.date) + ", " + pluralStringResource(R.plurals.home_cal_episodes, count, count)
            Surface(
                onClick = { onSelect(day.date) },
                shape = MaterialTheme.shapes.medium,
                color = if (isSel) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainer,
                modifier = Modifier.weight(1f).semantics {
                    contentDescription = description
                    this.selected = isSel
                },
            ) {
                Column(Modifier.padding(vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text(
                        day.date.dayOfWeek.getDisplayName(TextStyle.SHORT, Locale.getDefault()).take(3),
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                    )
                    Text(
                        day.date.dayOfMonth.toString(),
                        style = MaterialTheme.typography.titleMedium,
                        fontWeight = if (day.date == today) FontWeight.Bold else FontWeight.Normal,
                        color = if (day.date == today) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                    )
                    Text(
                        if (count > 0) count.toString() else "·",
                        style = MaterialTheme.typography.labelSmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
        }
    }
}

/** Wide week: seven columns side by side, each with compact episode cards; the row scrolls when narrower than 7 × 140 dp. */
@Composable
private fun WeekColumns(days: List<CalendarDay>, ctx: CalendarCtx) {
    val today = LocalDate.now()
    BoxWithConstraints(Modifier.fillMaxWidth().padding(horizontal = 12.dp)) {
        val colWidth = ((maxWidth - 48.dp) / 7).coerceAtLeast(140.dp)
        Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            days.forEach { day ->
                Column(Modifier.width(colWidth), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text(
                        dayTitle(day.date),
                        style = MaterialTheme.typography.labelLarge,
                        color = if (day.date == today) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.padding(vertical = 4.dp),
                    )
                    if (day.episodes.isEmpty()) {
                        Text("—", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.outline)
                    }
                    day.episodes.forEach { ep -> WeekCard(ep, ctx) }
                }
            }
        }
    }
}

@Composable
private fun WeekCard(ep: AiringEpisode, ctx: CalendarCtx) {
    val tracked = ctx.state.isTracked(ep.media.ref)
    Surface(
        onClick = { ctx.onOpen(ep.media) },
        shape = MaterialTheme.shapes.medium,
        color = if (tracked) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainer,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Row(Modifier.padding(6.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            MediaCover(url = ep.media.cover, modifier = Modifier.width(32.dp))
            Column(Modifier.weight(1f)) {
                Text(ctx.timeOf(ep), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.primary)
                Text(
                    ep.media.displayTitle(LocalTitleLanguage.current),
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    stringResource(UiR.string.ui_episode_number, ep.episode),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

/**
 * Month grid: weekday initials and one cell per day (days of the neighbour months dimmed). Phone cells show the day number and up
 * to three dots (tracked shows in the primary color); wide cells up to three small posters. Tapping selects the day.
 */
@Composable
private fun MonthGrid(days: List<CalendarDay>, selected: LocalDate, wide: Boolean, ctx: CalendarCtx, onSelect: (LocalDate) -> Unit) {
    val today = LocalDate.now()
    val month = selected.month
    Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Row(Modifier.fillMaxWidth()) {
            days.take(7).forEach { d ->
                Text(
                    d.date.dayOfWeek.getDisplayName(TextStyle.NARROW, Locale.getDefault()),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.weight(1f),
                )
            }
        }
        days.chunked(7).forEach { week ->
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                week.forEach { day ->
                    val isSel = day.date == selected
                    val inMonth = day.date.month == month
                    val count = day.episodes.size
                    val description = dayTitle(day.date) + ", " + pluralStringResource(R.plurals.home_cal_episodes, count, count)
                    Box(
                        Modifier
                            .weight(1f)
                            .aspectRatio(if (wide) 0.9f else 0.8f)
                            .clip(MaterialTheme.shapes.small)
                            .background(if (isSel) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainer)
                            .then(if (day.date == today) Modifier.border(1.dp, MaterialTheme.colorScheme.primary, MaterialTheme.shapes.small) else Modifier)
                            .clickable { onSelect(day.date) }
                            .semantics {
                                contentDescription = description
                                this.selected = isSel
                            }
                            .padding(4.dp),
                    ) {
                        Column(Modifier.fillMaxSize(), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text(
                                day.date.dayOfMonth.toString(),
                                style = MaterialTheme.typography.labelMedium,
                                color = if (inMonth) MaterialTheme.colorScheme.onSurface else MaterialTheme.colorScheme.outline,
                            )
                            if (wide) {
                                Row(horizontalArrangement = Arrangement.spacedBy((-10).dp), modifier = Modifier.padding(top = 4.dp)) {
                                    day.episodes.take(3).forEach { ep -> MediaCover(url = ep.media.cover, modifier = Modifier.width(26.dp)) }
                                }
                                if (count > 3) Text("+${count - 3}", style = MaterialTheme.typography.labelSmall)
                            } else if (count > 0) {
                                Row(horizontalArrangement = Arrangement.spacedBy(2.dp), modifier = Modifier.padding(top = 4.dp)) {
                                    day.episodes.take(3).forEach { ep ->
                                        val color = if (ctx.state.isTracked(ep.media.ref)) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.outline
                                        Box(Modifier.size(5.dp).clip(CircleShape).background(color))
                                    }
                                }
                                Text(count.toString(), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Hoy: hour headings, the episodes, and the "now" line. */
private fun LazyListScope.dayTimeline(episodes: List<AiringEpisode>, day: LocalDate, ctx: CalendarCtx) {
    if (episodes.isEmpty()) {
        emptyItem(this, "day-empty", ctx.state.onlyFollowed, whole = false)
        return
    }
    val lines = CalendarLogic.dayLines(episodes, day, ctx.zone, System.currentTimeMillis() / 1000)
    lines.forEachIndexed { i, line ->
        when (line) {
            is CalendarLogic.DayLine.Hour -> item(key = "h:${line.hour}", contentType = "hour") {
                Text(
                    "%02d:00".format(line.hour),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 16.dp, top = 12.dp, bottom = 4.dp),
                )
            }
            is CalendarLogic.DayLine.Ep -> {
                val ep = line.episode
                item(key = "t:$i:${ep.media.ref.anilistId}:${ep.episode}", contentType = "episode") {
                    CalendarRow(
                        ep = ep,
                        time = ctx.timeOf(ep),
                        tracked = ctx.state.isTracked(ep.media.ref),
                        followed = ctx.state.isFollowed(ep.media.ref),
                        onClick = { ctx.onOpen(ep.media) },
                        onToggleFollow = { ctx.onToggleFollow(ep) },
                    )
                }
            }
            CalendarLogic.DayLine.Now -> item(key = "now", contentType = "now") { NowMarker() }
        }
    }
}

@Composable
private fun NowMarker() {
    val label = stringResource(R.string.home_cal_now)
    Row(
        Modifier.fillMaxWidth().padding(horizontal = 12.dp, vertical = 6.dp).semantics { contentDescription = label },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Box(Modifier.size(10.dp).clip(CircleShape).background(MaterialTheme.colorScheme.error))
        Text(label, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.error)
        HorizontalDivider(Modifier.weight(1f), thickness = 2.dp, color = MaterialTheme.colorScheme.error)
    }
}
