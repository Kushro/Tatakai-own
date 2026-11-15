// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.model.Episode
import app.tatakai.next.model.HistoryEntry
import app.tatakai.next.ui.format.relativeAirDay
import app.tatakai.next.ui.look.pressScale
import app.tatakai.next.ui.media.progressFraction
import coil3.compose.AsyncImage
import java.util.Locale
import app.tatakai.next.ui.R as UiR

// 2026-11 UI refresh: Details › Episodios views (desktop episodes.rs, Hayase-like cards). CARDS = 16:9 thumbnail with length and
// rating, "N. title", two lines of synopsis, relative date and the watched bar; COMPACT = number + title cells. LIST stays the
// thumbnail row of DetailsSections (with the same meta line). Without a thumbnail the series art shows dimmed with the number, never
// an empty box; unaired episodes show "en 6 días" and cannot be opened.

/** "8.03" -> "8.0" (one decimal, the locale's separator). */
internal fun ratingLabel(rating: Double?, locale: Locale = Locale.getDefault()): String? =
    rating?.takeIf { it > 0 }?.let { String.format(locale, "%.1f", it) }

/** Meta line of the list / card views: relative date · length (· rating when there is no thumbnail badge for it). */
@Composable
internal fun episodeMeta(episode: Episode, withRating: Boolean): String? {
    val parts = buildList {
        relativeAirDay(episode.airDate)?.let { add(it.replaceFirstChar { c -> c.titlecase() }) }
        episode.runtimeMin?.let { add(stringResource(UiR.string.ui_minutes_short, it)) }
        if (withRating) ratingLabel(episode.rating)?.let { add("★ $it") }
    }
    return parts.takeIf { it.isNotEmpty() }?.joinToString(" · ")
}

/** Thumbnail, or the series art dimmed with the episode number on top. */
@Composable
internal fun EpisodeThumb(episode: Episode, fallbackArt: String?, modifier: Modifier = Modifier) {
    Box(modifier.aspectRatio(16f / 9f).clip(MaterialTheme.shapes.medium).background(MaterialTheme.colorScheme.surfaceContainerHighest)) {
        val image = episode.image?.takeIf { it.isNotBlank() }
        if (image != null) {
            AsyncImage(model = image, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize())
        } else {
            if (!fallbackArt.isNullOrBlank()) {
                AsyncImage(
                    model = fallbackArt,
                    contentDescription = null,
                    contentScale = ContentScale.Crop,
                    modifier = Modifier.fillMaxSize().alpha(0.35f),
                )
            }
            Text(
                episode.number.toString(),
                style = MaterialTheme.typography.headlineMedium,
                color = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.align(Alignment.Center),
            )
        }
    }
}

@Composable
private fun Badge(text: String, modifier: Modifier = Modifier, star: Boolean = false) {
    Row(
        modifier
            .clip(CircleShape)
            .background(Color.Black.copy(alpha = 0.62f))
            .padding(horizontal = 6.dp, vertical = 2.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        if (star) Icon(Icons.Filled.Star, contentDescription = null, tint = Color(0xFFFFC94D), modifier = Modifier.size(12.dp))
        Text(text, style = MaterialTheme.typography.labelSmall, color = Color.White)
    }
}

/** CARDS view: one large card (phones: full width; wide windows: two per row). */
@Composable
internal fun EpisodeCard(
    episode: Episode,
    unaired: Boolean,
    progress: HistoryEntry?,
    watched: Boolean,
    current: Boolean,
    downloaded: Boolean,
    fallbackArt: String?,
    onClick: () -> Unit,
    onDownload: () -> Unit,
    onToggleWatched: (() -> Unit)?,
    modifier: Modifier = Modifier,
) {
    val interaction = remember { MutableInteractionSource() }
    val title = episode.title?.takeIf { it.isNotBlank() }
    val heading = if (title != null) {
        stringResource(R.string.details_episode_title, episode.number, title)
    } else {
        stringResource(UiR.string.ui_episode_number, episode.number)
    }
    Surface(
        onClick = onClick,
        enabled = !unaired,
        interactionSource = interaction,
        shape = MaterialTheme.shapes.large,
        color = if (current) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainer,
        border = if (current) BorderStroke(2.dp, MaterialTheme.colorScheme.primary) else null,
        modifier = modifier.pressScale(interaction, 0.98f).then(if (unaired) Modifier.alpha(0.6f) else Modifier),
    ) {
        Column {
            Box {
                EpisodeThumb(episode, fallbackArt, Modifier.fillMaxWidth())
                ratingLabel(episode.rating)?.let { Badge(it, Modifier.align(Alignment.TopStart).padding(8.dp), star = true) }
                episode.runtimeMin?.let {
                    Badge(stringResource(UiR.string.ui_minutes_short, it), Modifier.align(Alignment.BottomEnd).padding(8.dp))
                }
                val fraction = progress?.takeIf { !it.completed && !watched }?.progressFraction()
                if (fraction != null) {
                    LinearProgressIndicator(
                        progress = { fraction },
                        modifier = Modifier.fillMaxWidth().align(Alignment.BottomCenter),
                    )
                }
            }
            Row(Modifier.padding(start = 12.dp, top = 10.dp, end = 4.dp, bottom = 10.dp), verticalAlignment = Alignment.Top) {
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(
                        heading,
                        style = MaterialTheme.typography.titleSmall,
                        color = if (current) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                    episode.summary?.takeIf { it.isNotBlank() }?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                    val date = relativeAirDay(episode.airDate)?.replaceFirstChar { it.titlecase() }
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                        if (date != null) {
                            Text(date, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                        }
                        if (unaired && date == null) Pill(stringResource(R.string.details_unaired), tone = PillTone.NEUTRAL)
                        if (episode.isFiller) Pill(stringResource(R.string.details_filler), tone = PillTone.WARN)
                        if (downloaded) Pill(stringResource(UiR.string.download_state_completed), tone = PillTone.NEUTRAL)
                    }
                }
                if (!unaired && onToggleWatched != null) {
                    IconButton(onClick = onToggleWatched) {
                        Icon(
                            if (watched) Icons.Filled.CheckCircle else Icons.Outlined.CheckCircle,
                            contentDescription = stringResource(
                                if (watched) R.string.details_mark_unwatched else R.string.details_mark_watched,
                                episode.number,
                            ),
                            tint = if (watched) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        )
                    }
                }
                if (!unaired) {
                    IconButton(onClick = onDownload) {
                        Icon(Icons.Outlined.Download, contentDescription = stringResource(R.string.details_download_episode, episode.number))
                    }
                }
            }
        }
    }
}

/** COMPACT view: number and title on one line; watched = tonal, current = primary outline, unaired = dimmed and not clickable. */
@Composable
internal fun CompactEpisodeLine(
    item: EpisodeItem,
    watched: Boolean,
    current: Boolean,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val container = when {
        watched -> MaterialTheme.colorScheme.secondaryContainer
        item.episode.isFiller -> MaterialTheme.colorScheme.tertiaryContainer
        else -> MaterialTheme.colorScheme.surfaceContainerHigh
    }
    val title = item.episode.title?.takeIf { it.isNotBlank() }
    val description = if (item.unaired) {
        stringResource(R.string.details_episode_unaired_cd, item.episode.number)
    } else if (title != null) {
        stringResource(R.string.details_episode_title, item.episode.number, title)
    } else {
        stringResource(UiR.string.ui_episode_number, item.episode.number)
    }
    Surface(
        onClick = onClick,
        enabled = !item.unaired,
        shape = MaterialTheme.shapes.small,
        color = container,
        border = if (current) BorderStroke(2.dp, MaterialTheme.colorScheme.primary) else null,
        modifier = modifier.heightIn(min = 44.dp).then(if (item.unaired) Modifier.alpha(0.5f) else Modifier).semantics(mergeDescendants = true) {
            contentDescription = description
        },
    ) {
        Row(Modifier.padding(horizontal = 10.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Text(
                item.episode.number.toString(),
                style = MaterialTheme.typography.labelLarge,
                color = if (current) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
            )
            if (title != null) {
                Text(title, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            }
            if (watched) Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(14.dp))
        }
    }
}
