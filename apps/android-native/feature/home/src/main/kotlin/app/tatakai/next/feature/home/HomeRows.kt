// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.AutoStories
import androidx.compose.material.icons.outlined.CalendarMonth
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.Explore
import androidx.compose.material.icons.automirrored.outlined.MenuBook
import androidx.compose.material.icons.outlined.Notifications
import androidx.compose.material3.AssistChip
import androidx.compose.material3.Badge
import androidx.compose.material3.BadgedBox
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.model.AiringEpisode
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.RecommendationItem
import app.tatakai.next.model.SocialRepository
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.mediaKey
import app.tatakai.next.ui.state.userMessage
import kotlinx.coroutines.flow.MutableStateFlow
import org.koin.compose.getKoin
import kotlin.math.roundToInt

// Parity wave B (16 §3.1, worker r): the new rows of Home. Every row loads, fails and retries on its own (inline compact error).

/** Badge of the bell: `SocialRepository.unreadCount` when the social module is loaded (t), else 0. */
@Composable
internal fun rememberUnreadNotifications(): Int {
    val koin = getKoin()
    val flow = remember { koin.getOrNull<SocialRepository>()?.unreadCount ?: MutableStateFlow(0) }
    val count by flow.collectAsStateWithLifecycle()
    return count
}

@Composable
internal fun NotificationBell(unread: Int, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        BadgedBox(badge = { if (unread > 0) Badge { Text(if (unread > 99) "99+" else unread.toString()) } }) {
            Icon(Icons.Outlined.Notifications, contentDescription = stringResource(R.string.home_notifications))
        }
    }
}

/** Quick chips of the greeting: Calendario · Para ti · Manga · Novelas (phones reach the readers from here, 16 §0.11). */
@Composable
internal fun QuickChips(actions: HomeActions) {
    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        QuickChip(stringResource(R.string.home_chip_calendar), Icons.Outlined.CalendarMonth, actions.onOpenCalendar)
        QuickChip(stringResource(R.string.home_for_you_title), Icons.Outlined.Explore, actions.onOpenForYou)
        QuickChip(stringResource(UiR.string.nav_manga), Icons.AutoMirrored.Outlined.MenuBook, actions.onOpenManga)
        QuickChip(stringResource(UiR.string.nav_novels), Icons.Outlined.AutoStories, actions.onOpenNovels)
    }
}

@Composable
private fun QuickChip(label: String, icon: androidx.compose.ui.graphics.vector.ImageVector, onClick: () -> Unit) {
    AssistChip(
        onClick = onClick,
        label = { Text(label) },
        leadingIcon = { Icon(icon, contentDescription = null, modifier = Modifier.size(18.dp)) },
    )
}

/** "Descargando": active torrents (q) + web downloads (r), checklist home.torrent-activity. */
@Composable
internal fun DownloadingRow(activity: DownloadActivity) {
    Column(Modifier.fillMaxWidth()) {
        SectionHeader(stringResource(R.string.home_downloading, activity.total), onSeeAll = null)
        LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            items(activity.web, key = { "w:" + it.id }) { item ->
                DownloadChip(item.poster, item.title, stringResource(R.string.home_downloading_episode, item.episode), item.progress.toFloat())
            }
            items(activity.torrents, key = { "t:" + it.id.toString() }) { t ->
                DownloadChip(t.poster, t.series ?: t.name, t.episode?.let { stringResource(R.string.home_downloading_episode, it) } ?: stringResource(R.string.home_downloading_torrent), t.progress.toFloat())
            }
        }
    }
}

@Composable
private fun DownloadChip(poster: String?, title: String, subtitle: String, progress: Float) {
    Surface(shape = MaterialTheme.shapes.large, color = MaterialTheme.colorScheme.surfaceContainer, modifier = Modifier.width(240.dp)) {
        Row(Modifier.padding(8.dp), horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
            MediaCover(url = poster, modifier = Modifier.width(40.dp))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(title, style = MaterialTheme.typography.titleSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    Icon(Icons.Outlined.Download, contentDescription = null, tint = MaterialTheme.colorScheme.primary)
                    Text(
                        "$subtitle · ${(progress.coerceIn(0f, 1f) * 100).roundToInt()}%",
                        style = MaterialTheme.typography.labelMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
                ProgressLine(progress)
            }
        }
    }
}

/** Generic media row of the wave B rows: skeleton / compact error with retry / nothing when empty. */
@Composable
private fun <T> DiscoverRow(
    title: String,
    state: LoadState<List<T>>,
    onRetry: () -> Unit,
    onSeeAll: (() -> Unit)?,
    header: @Composable () -> Unit = {},
    hideWhenEmpty: Boolean = true,
    emptyText: String? = null,
    content: LazyListScope.(List<T>) -> Unit,
) {
    if (hideWhenEmpty && state is LoadState.Ok && state.value.isEmpty()) return
    Column(Modifier.fillMaxWidth()) {
        SectionHeader(title, onSeeAll)
        header()
        when (state) {
            LoadState.Loading -> SkeletonRow()
            is LoadState.Err -> ErrorState(
                message = state.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = onRetry,
                compact = true,
            )
            is LoadState.Ok -> if (state.value.isEmpty()) {
                if (emptyText != null) {
                    Text(
                        emptyText,
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
                    )
                }
            } else {
                LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    content(state.value)
                }
            }
        }
    }
}

/** Rows above the base feeds: followed updates, latest episodes, Top 10, For you, Because you watched. */
internal fun LazyListScope.homeDiscoverTop(d: HomeDiscoverState, gate: MatureGate, actions: HomeActions) {
    item(key = "followed", contentType = "discover") {
        DiscoverRow(
            title = stringResource(R.string.home_followed_title),
            state = d.followed,
            onRetry = actions.onRetryFollowed,
            onSeeAll = actions.onOpenCalendar,
        ) { list -> airingItems("fol", list, gate, actions) }
    }
    item(key = "latest", contentType = "discover") {
        DiscoverRow(
            title = stringResource(R.string.home_latest_title),
            state = d.latest,
            onRetry = actions.onRetryLatest,
            onSeeAll = actions.onOpenCalendar,
        ) { list -> airingItems("lat", list, gate, actions) }
    }
    item(key = "top10", contentType = "discover") {
        DiscoverRow(
            title = stringResource(R.string.home_top10_title),
            state = d.top10,
            onRetry = { actions.onTopWindow(d.topWindow) },
            onSeeAll = null,
            hideWhenEmpty = false,
            emptyText = stringResource(R.string.home_row_empty),
            header = {
                Segmented(
                    options = listOf(
                        SegmentedOption(TopWindow.TODAY, stringResource(R.string.home_top10_today)),
                        SegmentedOption(TopWindow.WEEK, stringResource(R.string.home_top10_week)),
                        SegmentedOption(TopWindow.MONTH, stringResource(R.string.home_top10_month)),
                    ),
                    selected = d.topWindow,
                    onSelect = actions.onTopWindow,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
                )
            },
        ) { list ->
            itemsIndexed(list, key = { _, m -> "top:" + mediaKey(m) }) { index, media ->
                RankedCard(index + 1, media, gate) { actions.onMediaClick(media) }
            }
        }
    }
    item(key = "for-you", contentType = "discover") {
        DiscoverRow(
            title = stringResource(R.string.home_for_you_title),
            state = d.forYou,
            onRetry = actions.onRetryForYou,
            onSeeAll = actions.onOpenForYou,
        ) { list ->
            items(list, key = { "fy:" + mediaKey(it.media) }) { item: RecommendationItem ->
                GatedMediaCard(
                    media = item.media,
                    gate = gate,
                    onClick = { actions.onMediaClick(item.media) },
                    width = CARD_WIDTH,
                    subtitle = stringResource(R.string.home_recs_affinity, (item.affinity * 100).roundToInt()),
                )
            }
        }
    }
    d.because?.let { because ->
        item(key = "because", contentType = "discover") {
            DiscoverRow(
                title = stringResource(R.string.home_because_title, because.sourceTitle),
                state = because.state,
                onRetry = actions.onRetryBecause,
                onSeeAll = null,
            ) { list -> mediaItems("bec", list, gate, actions) }
        }
    }
}

/** Rows below the base feeds: upcoming premieres (with season / year) and most favourited. */
internal fun LazyListScope.homeDiscoverBottom(d: HomeDiscoverState, gate: MatureGate, actions: HomeActions) {
    item(key = "upcoming", contentType = "discover") {
        DiscoverRow(
            title = stringResource(R.string.home_upcoming_title),
            state = d.upcoming,
            onRetry = actions.onRetryUpcoming,
            onSeeAll = null,
        ) { list ->
            items(list, key = { "up:" + mediaKey(it) }) { media ->
                GatedMediaCard(
                    media = media,
                    gate = gate,
                    onClick = { actions.onMediaClick(media) },
                    width = CARD_WIDTH,
                    subtitle = premiereLabel(media),
                )
            }
        }
    }
    item(key = "favourites", contentType = "discover") {
        DiscoverRow(
            title = stringResource(R.string.home_favourites_title),
            state = d.favourites,
            onRetry = actions.onRetryFavourites,
            onSeeAll = null,
        ) { list -> mediaItems("fav", list, gate, actions) }
    }
}

/** Infinite genre rows: each one appended when the sentinel at the bottom is composed (scrolled into view). */
internal fun LazyListScope.homeGenreRows(d: HomeDiscoverState, gate: MatureGate, actions: HomeActions, fallbackGenres: List<String>) {
    d.genreRows.forEach { row ->
        item(key = "genre-row:" + row.genre, contentType = "discover") {
            DiscoverRow(
                title = stringResource(R.string.home_genre_row, row.genre),
                state = row.state,
                onRetry = { actions.onRetryGenre(row.genre) },
                onSeeAll = { actions.onOpenGenre(row.genre) },
            ) { list -> mediaItems("g:" + row.genre, list, gate, actions) }
        }
    }
    if (!d.genreRowsDone) {
        item(key = "genre-sentinel:" + d.genreRows.size, contentType = "sentinel") {
            LaunchedEffect(d.genreRows.size) { actions.onNextGenreRow(fallbackGenres) }
            Box(Modifier.fillMaxWidth().padding(8.dp))
        }
    }
}

private fun LazyListScope.mediaItems(prefix: String, list: List<Media>, gate: MatureGate, actions: HomeActions) {
    items(list, key = { "$prefix:" + mediaKey(it) }) { media ->
        GatedMediaCard(media = media, gate = gate, onClick = { actions.onMediaClick(media) }, width = CARD_WIDTH)
    }
}

private fun LazyListScope.airingItems(prefix: String, list: List<AiringEpisode>, gate: MatureGate, actions: HomeActions) {
    items(list, key = { "$prefix:" + mediaKey(it.media) + ":" + it.episode }) { ep ->
        GatedMediaCard(
            media = ep.media,
            gate = gate,
            onClick = { actions.onMediaClick(ep.media) },
            width = CARD_WIDTH,
            subtitle = stringResource(R.string.home_continue_episode, ep.episode),
        )
    }
}

@Composable
private fun RankedCard(rank: Int, media: Media, gate: MatureGate, onClick: () -> Unit) {
    Row(verticalAlignment = Alignment.Bottom) {
        Text(
            rank.toString(),
            style = MaterialTheme.typography.displayMedium,
            fontWeight = FontWeight.Black,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.padding(end = 4.dp, bottom = 40.dp),
        )
        GatedMediaCard(media = media, gate = gate, onClick = onClick, width = CARD_WIDTH)
    }
}

/** "Primavera 2027" / "2027" / "Pronto". */
@Composable
private fun premiereLabel(media: Media): String {
    val season = when (media.season?.uppercase()) {
        "WINTER" -> stringResource(R.string.home_season_winter)
        "SPRING" -> stringResource(R.string.home_season_spring)
        "SUMMER" -> stringResource(R.string.home_season_summer)
        "FALL" -> stringResource(R.string.home_season_fall)
        else -> null
    }
    val year = media.year
    return when {
        season != null && year != null -> "$season $year"
        year != null -> year.toString()
        else -> stringResource(R.string.home_premiere_tba)
    }
}
