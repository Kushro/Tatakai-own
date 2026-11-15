// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.BookmarkAdd
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material3.Button
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.model.ListStatus
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.Media
import app.tatakai.next.model.Motion
import app.tatakai.next.ui.labels.genreLabel
import app.tatakai.next.ui.look.lookMotion
import app.tatakai.next.ui.media.LocalTitleLanguage
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.media.dotColor
import app.tatakai.next.ui.media.label
import app.tatakai.next.ui.media.mediaFormatLabel
import app.tatakai.next.ui.media.mediaKey
import app.tatakai.next.ui.media.mediaStatusKind
import coil3.compose.AsyncImage
import kotlinx.coroutines.flow.flowOf
import kotlinx.coroutines.launch
import org.koin.compose.getKoin
import app.tatakai.next.ui.R as UiR

// 2026-11 UI refresh: Home "Destacado" (desktop HomeLayout::Spotlight, Hayase-like). A full-width pager of the trending titles drawn
// under the status bar: banner, fact chips (episodes, format, status, season, score), title, genres, synopsis, Continue / Watch,
// list button and details; a segmented progress bar that fills over the autoplay period and pauses while a finger is on the hero.
// Perf (moto g51): one image per page sized to the slot by Coil, the neighbour pre-composed (beyondViewportPageCount = 1) so its banner
// is already decoded, progress drawn by a single Animatable read in the draw phase; no blur.

/** Spotlight order: titles with a banner first (their order kept), the ones with only a poster at the end of the rotation. */
internal fun spotlightOrder(items: List<Media>): List<Media> = items.sortedBy { it.banner.isNullOrBlank() }

/** The history entry to continue [media] with, if the user already started it. */
internal fun continueEntryFor(media: Media, entries: List<HistoryEntry>): HistoryEntry? = entries.firstOrNull { e ->
    (media.ref.anilistId != null && e.anilistId == media.ref.anilistId) || (media.ref.malId != null && e.malId == media.ref.malId)
}

@Composable
internal fun Spotlight(
    items: List<Media>,
    continueWatching: List<HistoryEntry>,
    autoplayMs: Long?,
    onOpen: (Media) -> Unit,
    onWatch: (Media) -> Unit,
    onContinue: (HistoryEntry) -> Unit,
    modifier: Modifier = Modifier,
    topEnd: @Composable () -> Unit = {},
) {
    val ordered = remember(items) { spotlightOrder(items) }
    if (ordered.isEmpty()) return
    val pager = rememberPagerState { ordered.size }
    val motion = lookMotion()
    val scope = rememberCoroutineScope()
    var touching by remember { mutableStateOf(false) }
    val progress = remember { Animatable(0f) }
    val period = autoplayMs?.takeIf { motion != Motion.OFF && ordered.size > 1 }

    var timedPage by remember { mutableStateOf(-1) }
    // Restarts on every settled page and on touch start / end: a new page starts the bar from 0, a released finger continues it
    // from where it paused. (Not keyed on isScrollInProgress: that would cancel its own page animation.)
    LaunchedEffect(pager.settledPage, touching, period) {
        if (timedPage != pager.settledPage) {
            timedPage = pager.settledPage
            progress.snapTo(0f)
        }
        if (period == null || touching) return@LaunchedEffect
        val left = ((1f - progress.value) * period).toInt().coerceAtLeast(1)
        progress.animateTo(1f, tween(left, easing = LinearEasing))
        val next = (pager.settledPage + 1) % ordered.size
        if (motion == Motion.EXPRESSIVE) pager.animateScrollToPage(next) else pager.scrollToPage(next)
    }

    BoxWithConstraints(
        modifier
            .fillMaxWidth()
            // Observe (never consume) touches so the buttons and the pager keep working: a finger on the hero pauses the timer.
            .pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
                    touching = true
                    do {
                        val event = awaitPointerEvent(PointerEventPass.Initial)
                    } while (event.changes.any { it.pressed })
                    touching = false
                }
            },
    ) {
        val wide = maxWidth >= 600.dp
        val height = if (wide) 440.dp else 500.dp
        Box(Modifier.fillMaxWidth().height(height)) {
            HorizontalPager(
                state = pager,
                key = { mediaKey(ordered[it]) },
                beyondViewportPageCount = 1,
                modifier = Modifier.fillMaxSize(),
            ) { page ->
                val media = ordered[page]
                SpotlightPage(
                    media = media,
                    continueEntry = continueEntryFor(media, continueWatching),
                    wide = wide,
                    onOpen = { onOpen(media) },
                    onWatch = { onWatch(media) },
                    onContinue = onContinue,
                )
            }
            Box(Modifier.align(Alignment.TopEnd).windowInsetsPadding(WindowInsets.statusBars).padding(8.dp)) { topEnd() }
            if (ordered.size > 1) {
                SpotlightProgress(
                    count = ordered.size,
                    current = pager.currentPage,
                    progress = { if (period == null) 1f else progress.value },
                    onSelect = { i -> scope.launch { if (motion == Motion.EXPRESSIVE) pager.animateScrollToPage(i) else pager.scrollToPage(i) } },
                    modifier = Modifier.align(Alignment.BottomCenter).padding(horizontal = 16.dp, vertical = 8.dp),
                )
            }
        }
    }
}

@Composable
private fun SpotlightPage(
    media: Media,
    continueEntry: HistoryEntry?,
    wide: Boolean,
    onOpen: () -> Unit,
    onWatch: () -> Unit,
    onContinue: (HistoryEntry) -> Unit,
) {
    val surface = MaterialTheme.colorScheme.surface
    val title = media.displayTitle(LocalTitleLanguage.current)
    Box(Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHighest)) {
        AsyncImage(
            model = media.banner?.takeIf { it.isNotBlank() } ?: media.cover,
            contentDescription = null,
            contentScale = ContentScale.Crop,
            alignment = Alignment.TopCenter,
            modifier = Modifier.fillMaxWidth().fillMaxHeight(if (wide) 1f else 0.62f),
        )
        // Plain gradients (no blur): legible text on any banner, cheap to draw.
        Box(
            Modifier.fillMaxSize().background(
                Brush.verticalGradient(
                    0f to surface.copy(alpha = 0.55f),
                    0.18f to Color.Transparent,
                    0.42f to surface.copy(alpha = 0.35f),
                    0.66f to surface.copy(alpha = 0.92f),
                    1f to surface,
                ),
            ),
        )
        if (wide) {
            Box(Modifier.fillMaxSize().background(Brush.horizontalGradient(listOf(surface.copy(alpha = 0.9f), Color.Transparent))))
        }
        Column(
            Modifier
                .align(Alignment.BottomStart)
                .then(if (wide) Modifier.widthIn(max = 560.dp) else Modifier.fillMaxWidth())
                .clip(MaterialTheme.shapes.large)
                .padding(start = 16.dp, end = 16.dp, bottom = 28.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FactChips(media)
            Text(
                title,
                style = if (wide) MaterialTheme.typography.displaySmall else MaterialTheme.typography.headlineMedium,
                color = MaterialTheme.colorScheme.onSurface,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.semantics { contentDescription = title },
            )
            if (media.genres.isNotEmpty()) {
                Text(
                    media.genres.take(4).map { genreLabel(it) }.joinToString(" · "),
                    style = MaterialTheme.typography.labelLarge,
                    color = MaterialTheme.colorScheme.primary,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            media.synopsis?.takeIf { it.isNotBlank() }?.let {
                Text(
                    it,
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 3,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Button(onClick = { if (continueEntry != null) onContinue(continueEntry) else onWatch() }) {
                    Icon(Icons.Filled.PlayArrow, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(
                        if (continueEntry != null) {
                            stringResource(R.string.home_spot_continue, continueEntry.episode)
                        } else {
                            stringResource(R.string.home_spot_watch)
                        },
                        modifier = Modifier.padding(start = 6.dp),
                        maxLines = 1,
                    )
                }
                ListButton(media)
                FilledTonalIconButton(onClick = onOpen) {
                    Icon(Icons.Outlined.Info, contentDescription = stringResource(R.string.home_details))
                }
            }
        }
    }
}

/** Episodes, format, status (with its dot), season and score as small pills. */
@Composable
private fun FactChips(media: Media) {
    val season = seasonLabel(media.season)
    FlowRow(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        media.episodes?.takeIf { it > 0 }?.let { Fact(pluralStringResource(R.plurals.home_spot_episodes, it, it)) }
        mediaFormatLabel(media.format)?.let { Fact(it) }
        mediaStatusKind(media.status)?.let { Fact(it.label(), dot = it.dotColor()) }
        when {
            season != null && media.year != null -> Fact("$season ${media.year}")
            media.year != null -> Fact(media.year.toString())
        }
        media.score?.let { Fact("$it%", star = true) }
    }
}

@Composable
private fun Fact(text: String, dot: Color? = null, star: Boolean = false) {
    Surface(shape = CircleShape, color = MaterialTheme.colorScheme.surfaceContainerHigh.copy(alpha = 0.85f)) {
        Row(
            Modifier.padding(horizontal = 10.dp, vertical = 4.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            if (dot != null) Box(Modifier.size(8.dp).clip(CircleShape).background(dot))
            if (star) Icon(Icons.Filled.Star, contentDescription = null, modifier = Modifier.size(14.dp), tint = MaterialTheme.colorScheme.tertiary)
            Text(text, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.onSurface, maxLines = 1)
        }
    }
}

@Composable
private fun seasonLabel(season: String?): String? = when (season?.uppercase()) {
    "WINTER" -> stringResource(R.string.home_season_winter)
    "SPRING" -> stringResource(R.string.home_season_spring)
    "SUMMER" -> stringResource(R.string.home_season_summer)
    "FALL" -> stringResource(R.string.home_season_fall)
    else -> null
}

/**
 * List button: filled when the title is on the user's list (tap opens nothing new: the status shows in its description), otherwise
 * adds it as "Planeado" with a snackbar. Hidden when no ListsRepository is bound (tests / previews).
 */
@Composable
private fun ListButton(media: Media) {
    val koin = getKoin()
    val lists = remember(koin) { runCatching { koin.getOrNull<ListsRepository>() }.getOrNull() } ?: return
    val entryFlow = remember(lists, media.ref) { runCatching { lists.entry(media.ref) }.getOrDefault(flowOf(null)) }
    val entry by entryFlow.collectAsState(initial = null)
    val scope = rememberCoroutineScope()
    val snackbar = LocalAppSnackbar.current
    val added = stringResource(R.string.home_spot_added)
    val failed = stringResource(R.string.home_spot_add_failed)
    val onList = entry != null
    FilledTonalIconButton(
        onClick = {
            if (!onList) {
                scope.launch {
                    val ok = lists.setStatus(media, ListStatus.PLANNING).isSuccess
                    snackbar.show(if (ok) added else failed)
                }
            }
        },
    ) {
        Icon(
            if (onList) Icons.Filled.Bookmark else Icons.Outlined.BookmarkAdd,
            contentDescription = stringResource(if (onList) R.string.home_spot_on_list else R.string.home_spot_add),
        )
    }
}

/**
 * One segment per title: past ones full, the current one fills with [progress] (read in the draw phase), the rest empty. Each segment
 * is a 24 dp tall touch target that jumps to its title.
 */
@Composable
private fun SpotlightProgress(
    count: Int,
    current: Int,
    progress: () -> Float,
    onSelect: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val track = MaterialTheme.colorScheme.onSurface.copy(alpha = 0.18f)
    val fill = MaterialTheme.colorScheme.primary
    Row(modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        repeat(count) { i ->
            val goTo = stringResource(UiR.string.ui_hero_go_to, i + 1, count)
            Box(
                Modifier
                    .weight(1f)
                    .height(24.dp)
                    .clip(CircleShape)
                    .semantics { contentDescription = goTo }
                    .clickable { onSelect(i) },
                contentAlignment = Alignment.Center,
            ) {
                Box(
                    Modifier
                        .fillMaxWidth()
                        .height(3.dp)
                        .clip(CircleShape)
                        .background(track)
                        .drawBehind {
                            val f = when {
                                i < current -> 1f
                                i == current -> progress().coerceIn(0f, 1f)
                                else -> 0f
                            }
                            if (f > 0f) drawRect(fill, size = size.copy(width = size.width * f))
                        },
                )
            }
        }
    }
}
