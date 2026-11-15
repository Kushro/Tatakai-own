// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.media

import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Star
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.graphics.Color
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.tatakai.next.model.CardSize
import app.tatakai.next.model.Media
import app.tatakai.next.ui.R
import app.tatakai.next.ui.labels.genreLabel
import app.tatakai.next.ui.look.Ago
import app.tatakai.next.ui.look.CardSizing
import app.tatakai.next.ui.look.pressScale
import java.util.Locale

/**
 * 0.3.2: the horizontal "detailed" card of Tamaño de tarjetas › Grande / Extra grande (desktop `media.rs` large / xl): poster on the
 * left; on the right the title (2 lines), status dot + format · year · episodes, score, genres and, on episode rows, [episodeLine]
 * ("Episodio 5 · hace 3 horas") in the accent color. XL adds the synopsis when the data has one (rows do not ask for it). The whole
 * card is one TalkBack node: title, episode line and facts.
 */
@Composable
fun MediaWideCard(
    media: Media,
    title: String,
    onClick: () -> Unit,
    size: CardSize,
    width: Dp,
    modifier: Modifier = Modifier,
    episodeLine: String? = null,
    subtitle: String? = null,
) {
    val interaction = remember { MutableInteractionSource() }
    val xl = size == CardSize.XL
    val posterWidth = CardSizing.widePosterDp(size).dp
    val status = mediaStatusKind(media.status)
    val facts = listOfNotNull(
        status?.label(),
        mediaFormatLabel(media.format),
        media.year?.toString(),
        media.episodes?.takeIf { it > 0 }?.let { pluralStringResource(R.plurals.ui_episodes_short, it, it) },
    ).joinToString(" · ")
    val score = media.score?.let { String.format(Locale.ROOT, "%.1f", it / 10.0) }
    val genres = media.genres.take(if (xl) 3 else 2).map { genreLabel(it) }
    val spoken = listOfNotNull(title, episodeLine, subtitle, facts.ifBlank { null }, score?.let { stringResource(R.string.ui_card_score, it) })
        .joinToString(", ")
    Box(
        modifier
            .width(width)
            .pressScale(interaction)
            .clip(MaterialTheme.shapes.medium)
            .background(MaterialTheme.colorScheme.surfaceContainer)
            .clickable(interactionSource = interaction, indication = LocalIndication.current, onClick = onClick)
            .semantics(mergeDescendants = true) { contentDescription = spoken },
    ) {
        Row(Modifier.height(posterWidth / COVER_ASPECT_RATIO)) {
            MediaCover(url = media.cover, modifier = Modifier.width(posterWidth), shape = MaterialTheme.shapes.medium)
            Column(
                Modifier.weight(1f).padding(horizontal = 12.dp, vertical = 10.dp),
                verticalArrangement = Arrangement.spacedBy(4.dp),
            ) {
                Text(
                    title,
                    style = if (xl) MaterialTheme.typography.titleMedium else MaterialTheme.typography.titleSmall,
                    fontWeight = FontWeight.SemiBold,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
                (episodeLine ?: subtitle)?.let {
                    Text(
                        it,
                        style = MaterialTheme.typography.labelLarge,
                        color = if (episodeLine != null) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                        fontWeight = if (episodeLine != null) FontWeight.SemiBold else null,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (facts.isNotBlank() || score != null) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (status != null) {
                            Box(Modifier.padding(end = 6.dp).size(8.dp).clip(CircleShape).background(status.dotColor()))
                        }
                        if (facts.isNotBlank()) {
                            Text(
                                facts,
                                style = MaterialTheme.typography.labelMedium,
                                color = MaterialTheme.colorScheme.onSurfaceVariant,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f, fill = false),
                            )
                        }
                        if (score != null) {
                            Icon(
                                Icons.Filled.Star,
                                contentDescription = null,
                                tint = STAR,
                                modifier = Modifier.padding(start = if (facts.isNotBlank()) 8.dp else 0.dp, end = 2.dp).size(14.dp),
                            )
                            Text(score, style = MaterialTheme.typography.labelMedium, maxLines = 1)
                        }
                    }
                }
                if (genres.isNotEmpty()) {
                    Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        genres.forEach { g ->
                            Text(
                                g,
                                style = MaterialTheme.typography.labelSmall,
                                color = MaterialTheme.colorScheme.onSecondaryContainer,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier
                                    .weight(1f, fill = false)
                                    .clip(CircleShape)
                                    .background(MaterialTheme.colorScheme.secondaryContainer)
                                    .padding(horizontal = 8.dp, vertical = 2.dp),
                            )
                        }
                    }
                }
                if (xl) {
                    media.synopsis?.takeIf { it.isNotBlank() }?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant,
                            maxLines = 2,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }
    }
}

private val STAR = Color(0xFFFFC547)

/** "hace 3 horas" / "ahora mismo" for an event [secondsAgo] seconds ago. */
@Composable
fun agoLabel(secondsAgo: Long): String = when (val a = Ago.of(secondsAgo)) {
    Ago.Now -> stringResource(R.string.ui_ago_now)
    is Ago.Minutes -> pluralStringResource(R.plurals.ui_minutes_ago, a.n, a.n)
    is Ago.Hours -> pluralStringResource(R.plurals.ui_hours_ago, a.n, a.n)
    is Ago.Days -> pluralStringResource(R.plurals.ui_days_ago, a.n, a.n)
}

/** "Episodio 5 · hace 3 horas"; "Episodio 5" when the time is unknown. */
@Composable
fun episodeAgoLine(episode: Int, secondsAgo: Long?): String =
    if (secondsAgo == null) stringResource(R.string.ui_episode_n, episode) else stringResource(R.string.ui_episode_ago, episode, agoLabel(secondsAgo))
