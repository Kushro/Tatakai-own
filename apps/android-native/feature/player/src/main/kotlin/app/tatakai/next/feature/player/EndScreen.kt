// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.feature.player

import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.EmojiEvents
import androidx.compose.material.icons.rounded.FormatListNumbered
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Replay
import androidx.compose.material.icons.rounded.Sync
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialShapes
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.toShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.model.Episode
import app.tatakai.next.model.LinkedAccount
import app.tatakai.next.model.ListEntry
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListStatus
import app.tatakai.next.ui.media.MediaCover
import coil3.compose.AsyncImage

// 0.4.1 (user report 2026-11-17): "al llegar al final del video, una bonita pantalla post capítulo terminado" with the next
// episode, watch again, the episode list and the way back to the title's page. It sits over the last frame (the engines keep it)
// and replaces the small 0.3 next-episode card while it is up; the auto-next countdown is the same one (PlayerViewModel.next).

/** The next episode as the end screen shows it; [secondsLeft] null = nothing counts (auto-next off or cancelled). */
data class NextUpUi(
    val episode: Int,
    val title: String? = null,
    val image: String? = null,
    val secondsLeft: Int? = null,
    val totalSecs: Int = DEFAULT_COUNTDOWN_SECS,
    val sameTorrent: Boolean = false,
)

/**
 * What the end screen shows. [episode] null = not an episode of a known title (a PC library file: "Video terminado");
 * [next] null = nothing to move on to; [seriesFinished] = the last episode of a title with a known episode count.
 */
data class EndScreenUi(
    val episode: Int?,
    val next: NextUpUi?,
    val seriesFinished: Boolean,
    val canBrowseEpisodes: Boolean,
)

/**
 * The end screen for the engine state: null unless the episode played to its end (not looping, after a first frame).
 * [next] is the view model's countdown card (null when auto-next is off before it starts, or after Cancel); the next
 * episode's title / thumbnail come from the episode list when it is loaded.
 */
fun endScreenState(
    ended: Boolean,
    loop: Boolean,
    started: Boolean,
    episode: Int,
    totalEpisodes: Int?,
    hasNextEpisode: Boolean,
    canNavigateEpisodes: Boolean,
    next: NextEpisodeUi?,
    episodes: List<Episode> = emptyList(),
): EndScreenUi? {
    if (!ended || loop || !started) return null
    val hasNext = canNavigateEpisodes && hasNextEpisode
    val nextUp = if (hasNext) {
        val n = next?.episode ?: (episode + 1)
        val e = episodes.firstOrNull { it.number == n }
        NextUpUi(
            episode = n,
            title = e?.title?.takeIf { it.isNotBlank() },
            image = e?.image?.takeIf { it.isNotBlank() },
            secondsLeft = next?.secondsLeft,
            totalSecs = next?.totalSecs ?: DEFAULT_COUNTDOWN_SECS,
            sameTorrent = next?.sameTorrent ?: false,
        )
    } else {
        null
    }
    return EndScreenUi(
        episode = if (canNavigateEpisodes) episode else null,
        next = nextUp,
        seriesFinished = canNavigateEpisodes && !hasNext && totalEpisodes != null && totalEpisodes > 0 && episode >= totalEpisodes,
        canBrowseEpisodes = canNavigateEpisodes,
    )
}

/** What the end screen says about the progress. */
enum class EndProgressKind {
    /** Only the history: the episode counts as watched. */
    WATCHED,

    /** The list's progress moved to this episode (`AppSettings.autoListProgress`, title on the list). */
    LIST_UPDATED,

    /** That progress completed the title on the list. */
    LIST_COMPLETED,
}

data class EndProgressUi(val kind: EndProgressKind, val progress: Int = 0, val total: Int? = null, val syncsWith: List<ListProvider> = emptyList())

/**
 * The progress line from what is already stored (no network): [entry] is the list row of the title (`ListsRepository.entry`, it
 * changes once the history layer advanced it), [accounts] the linked list services. [trackable] false (no AniList / MAL / provider
 * id: nothing is recorded) -> null. A list row ahead of this episode (a rewatch) is left alone: only the history line.
 */
fun endProgress(trackable: Boolean, episode: Int, entry: ListEntry?, autoListProgress: Boolean, accounts: List<LinkedAccount>): EndProgressUi? {
    if (!trackable) return null
    if (entry == null || !autoListProgress || entry.progress != episode) return EndProgressUi(EndProgressKind.WATCHED)
    val syncs = accounts.map { it.provider }.distinct()
    val total = entry.totalEpisodes?.takeIf { it > 0 }
    val kind = if (entry.status == ListStatus.COMPLETED && total != null && episode >= total) EndProgressKind.LIST_COMPLETED else EndProgressKind.LIST_UPDATED
    return EndProgressUi(kind, progress = entry.progress, total = total, syncsWith = syncs)
}

/**
 * The full-screen end of episode. [onDetails] null = no title page to go to (the button then only goes back). [onHide] shows the
 * last frame and the controls again (the compact next card keeps counting); back / Esc still close the player.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun EpisodeEndScreen(
    ui: EndScreenUi,
    seriesTitle: String,
    poster: String?,
    progress: EndProgressUi?,
    onPlayNext: () -> Unit,
    onCancelCountdown: () -> Unit,
    onReplay: () -> Unit,
    onEpisodes: () -> Unit,
    onDetails: (() -> Unit)?,
    onBack: () -> Unit,
    onHide: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val scheme = MaterialTheme.colorScheme
    Box(
        modifier
            .fillMaxSize()
            // swallow taps: nothing under the screen (gestures, controls) reacts while it is up
            .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) {},
    ) {
        // Over the last frame: the poster blurred (Android 12+; older ones only dim it) and a scrim that keeps text readable.
        if (!poster.isNullOrBlank()) {
            val blurOk = Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
            AsyncImage(
                model = poster,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                alpha = if (blurOk) 0.55f else 0.25f,
                modifier = Modifier.fillMaxSize().then(if (blurOk) Modifier.blur(48.dp) else Modifier),
            )
        }
        Box(
            Modifier.fillMaxSize().background(
                Brush.verticalGradient(
                    0f to scheme.scrim.copy(alpha = 0.55f),
                    0.45f to scheme.background.copy(alpha = 0.78f),
                    1f to scheme.background.copy(alpha = 0.96f),
                ),
            ),
        )
        BoxWithConstraints(Modifier.fillMaxSize().statusBarsPadding().navigationBarsPadding()) {
            val wide = maxWidth >= WIDE_DP
            // A phone in landscape is ~360 dp tall: the cover leaves the header and the next card lays out in a row, so the play
            // button stays on screen without scrolling.
            val short = maxHeight < SHORT_DP
            Column(Modifier.fillMaxSize()) {
                // Top row: back (closes the player, like the system back) and "show the video"
                Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Rounded.ArrowBack, stringResource(R.string.player_back), tint = scheme.onBackground) }
                    Spacer(Modifier.weight(1f))
                    IconButton(onClick = onHide) { Icon(Icons.Rounded.Close, stringResource(R.string.player_end_hide), tint = scheme.onBackground) }
                }
                val scroll = rememberScrollState()
                val content = Modifier.fillMaxWidth().weight(1f).verticalScroll(scroll).padding(horizontal = if (wide) 48.dp else 20.dp)
                if (wide) {
                    Row(content.padding(bottom = 24.dp), Arrangement.spacedBy(40.dp), Alignment.CenterVertically) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(16.dp)) {
                            EndHeader(ui, seriesTitle, poster.takeUnless { short }, progress)
                            EndActions(ui, onReplay, onEpisodes, onDetails, onBack)
                        }
                        ui.next?.let { NextUpCard(it, onPlayNext, onCancelCountdown, short, Modifier.weight(1f).widthIn(max = 480.dp)) }
                            ?: FinishedCard(ui, onDetails, onBack, Modifier.weight(1f).widthIn(max = 480.dp))
                    }
                } else {
                    Column(content.padding(bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(20.dp)) {
                        EndHeader(ui, seriesTitle, poster, progress)
                        ui.next?.let { NextUpCard(it, onPlayNext, onCancelCountdown, short, Modifier.fillMaxWidth()) }
                            ?: FinishedCard(ui, onDetails, onBack, Modifier.fillMaxWidth())
                        EndActions(ui, onReplay, onEpisodes, onDetails, onBack)
                    }
                }
            }
        }
    }
}

/** Badge + "Episodio N terminado" + the series title + the progress line. */
@Composable
private fun EndHeader(ui: EndScreenUi, seriesTitle: String, poster: String?, progress: EndProgressUi?) {
    val scheme = MaterialTheme.colorScheme
    Row(horizontalArrangement = Arrangement.spacedBy(16.dp), verticalAlignment = Alignment.CenterVertically) {
        if (!poster.isNullOrBlank()) {
            MediaCover(url = poster, modifier = Modifier.width(72.dp), shape = MaterialTheme.shapes.large)
        }
        Column(Modifier.weight(1f, fill = false), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
                val finished = ui.seriesFinished
                Box(
                    Modifier.size(32.dp)
                        .clip((if (finished) MaterialShapes.Sunny else MaterialShapes.Cookie9Sided).toShape())
                        .background(if (finished) scheme.tertiary else scheme.primary),
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        if (finished) Icons.Rounded.EmojiEvents else Icons.Rounded.Check,
                        contentDescription = null,
                        tint = if (finished) scheme.onTertiary else scheme.onPrimary,
                        modifier = Modifier.size(18.dp),
                    )
                }
                Text(
                    when {
                        finished -> stringResource(R.string.player_end_series_done)
                        ui.episode != null -> stringResource(R.string.player_end_episode_done, ui.episode)
                        else -> stringResource(R.string.player_end_video_done)
                    },
                    style = MaterialTheme.typography.titleMedium,
                    color = if (finished) scheme.tertiary else scheme.primary,
                    fontWeight = FontWeight.SemiBold,
                )
            }
            Text(
                seriesTitle,
                style = MaterialTheme.typography.headlineSmall,
                color = scheme.onBackground,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            progress?.let { ProgressLine(it) }
        }
    }
}

@Composable
private fun ProgressLine(p: EndProgressUi) {
    val scheme = MaterialTheme.colorScheme
    val text = when (p.kind) {
        EndProgressKind.WATCHED -> stringResource(R.string.player_end_watched)
        EndProgressKind.LIST_COMPLETED -> stringResource(R.string.player_end_list_completed)
        EndProgressKind.LIST_UPDATED -> p.total?.let { stringResource(R.string.player_end_list_updated_total, p.progress, it) }
            ?: stringResource(R.string.player_end_list_updated, p.progress)
    }
    val sync = p.syncsWith.takeIf { it.isNotEmpty() && p.kind != EndProgressKind.WATCHED }?.joinToString(" · ") { providerName(it) }
    Surface(shape = CircleShape, color = scheme.secondaryContainer, contentColor = scheme.onSecondaryContainer) {
        Row(Modifier.padding(horizontal = 12.dp, vertical = 6.dp), Arrangement.spacedBy(6.dp), Alignment.CenterVertically) {
            Icon(if (sync != null) Icons.Rounded.Sync else Icons.Rounded.Check, contentDescription = null, modifier = Modifier.size(16.dp))
            Text(
                if (sync != null) "$text · ${stringResource(R.string.player_end_list_sync, sync)}" else text,
                style = MaterialTheme.typography.labelLarge,
                maxLines = 2,
            )
        }
    }
}

private fun providerName(p: ListProvider): String = when (p) {
    ListProvider.ANILIST -> "AniList"
    ListProvider.MAL -> "MyAnimeList"
}

/** "A continuación": the next episode's thumbnail with the countdown ring, its title and the big play button. */
@Composable
private fun NextUpCard(next: NextUpUi, onPlayNext: () -> Unit, onCancelCountdown: () -> Unit, short: Boolean, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val thumb: @Composable (Modifier) -> Unit = { m ->
        Box(m.clip(MaterialTheme.shapes.large).clickable(onClick = onPlayNext), contentAlignment = Alignment.Center) {
            MediaCover(url = next.image, aspectRatio = 16f / 9f, shape = MaterialTheme.shapes.large, modifier = Modifier.fillMaxSize())
            Box(Modifier.fillMaxSize().background(scheme.scrim.copy(alpha = 0.35f)))
            Surface(shape = CircleShape, color = scheme.surfaceContainerHighest.copy(alpha = 0.9f), contentColor = scheme.onSurface) {
                val left = next.secondsLeft
                val ring = if (short) 40.dp else 56.dp
                if (left != null) {
                    CountdownRing(left, next.totalSecs, Modifier.padding(6.dp), size = ring)
                } else {
                    Icon(Icons.Rounded.PlayArrow, contentDescription = null, modifier = Modifier.padding(10.dp).size(ring - 12.dp))
                }
            }
        }
    }
    Surface(modifier, shape = MaterialTheme.shapes.extraLarge, color = scheme.surfaceContainerHigh.copy(alpha = 0.92f), contentColor = scheme.onSurface) {
        Column(Modifier.padding(16.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            if (short) {
                Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                    thumb(Modifier.width(160.dp).aspectRatio(16f / 9f))
                    NextUpText(next, Modifier.weight(1f))
                }
            } else {
                thumb(Modifier.fillMaxWidth().heightIn(max = 220.dp).aspectRatio(16f / 9f, matchHeightConstraintsFirst = true).align(Alignment.CenterHorizontally))
                NextUpText(next)
            }
            NextUpButtons(next, onPlayNext, onCancelCountdown)
        }
    }
}

@Composable
private fun NextUpText(next: NextUpUi, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Column(modifier, verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    next.secondsLeft?.let { stringResource(R.string.player_end_next_in, it) } ?: stringResource(R.string.player_end_up_next),
                    style = MaterialTheme.typography.labelLarge,
                    color = scheme.primary,
                )
                Text(stringResource(R.string.player_next_episode_n, next.episode), style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.SemiBold)
                next.title?.let { Text(it, style = MaterialTheme.typography.bodyMedium, color = scheme.onSurfaceVariant, maxLines = 2, overflow = TextOverflow.Ellipsis) }
                if (next.sameTorrent) {
                    Text(stringResource(R.string.player_next_same_torrent), style = MaterialTheme.typography.labelMedium, color = scheme.tertiary)
                }
    }
}

@Composable
private fun NextUpButtons(next: NextUpUi, onPlayNext: () -> Unit, onCancelCountdown: () -> Unit) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Button(
                    onClick = onPlayNext,
                    modifier = Modifier.weight(1f).heightIn(min = BIG_BUTTON_DP),
                    contentPadding = ButtonDefaults.ButtonWithIconContentPadding,
                ) {
                    Icon(Icons.Rounded.PlayArrow, contentDescription = null, modifier = Modifier.size(24.dp))
                    Text(
                        stringResource(R.string.player_end_play_next, next.episode),
                        Modifier.padding(start = 8.dp),
                        style = MaterialTheme.typography.titleMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (next.secondsLeft != null) {
                    TextButton(onClick = onCancelCountdown, modifier = Modifier.heightIn(min = BIG_BUTTON_DP)) {
                        Text(stringResource(R.string.player_cancel))
                    }
                }
            }
}

/** Last episode: "Terminaste la serie" + the title's page (rating, relations) as the main way on; else (no next) just the way back. */
@Composable
private fun FinishedCard(ui: EndScreenUi, onDetails: (() -> Unit)?, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Surface(modifier, shape = MaterialTheme.shapes.extraLarge, color = scheme.surfaceContainerHigh.copy(alpha = 0.92f), contentColor = scheme.onSurface) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            if (ui.seriesFinished) {
                Text(stringResource(R.string.player_end_series_done_body), style = MaterialTheme.typography.bodyLarge, color = scheme.onSurfaceVariant)
            }
            val label = if (onDetails != null) R.string.player_end_details_open else R.string.player_back
            Button(onClick = onDetails ?: onBack, modifier = Modifier.fillMaxWidth().heightIn(min = BIG_BUTTON_DP)) {
                Icon(if (onDetails != null) Icons.Rounded.Info else Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = null, modifier = Modifier.size(22.dp))
                Text(stringResource(label), Modifier.padding(start = 8.dp), style = MaterialTheme.typography.titleMedium)
            }
        }
    }
}

/** "Volver a ver", "Episodios", "Volver a la ficha": big tonal / outlined buttons that wrap on a narrow screen. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun EndActions(ui: EndScreenUi, onReplay: () -> Unit, onEpisodes: () -> Unit, onDetails: (() -> Unit)?, onBack: () -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        EndButton(Icons.Rounded.Replay, stringResource(R.string.player_end_replay), onReplay, tonal = true)
        if (ui.canBrowseEpisodes) EndButton(Icons.Rounded.FormatListNumbered, stringResource(R.string.player_episodes), onEpisodes, tonal = true)
        // On the last episode the card above already leads to the page
        if (ui.next != null || !ui.seriesFinished) {
            if (onDetails != null) {
                EndButton(Icons.Rounded.Info, stringResource(R.string.player_end_details), onDetails, tonal = false)
            } else if (ui.next != null) {
                EndButton(Icons.AutoMirrored.Rounded.ArrowBack, stringResource(R.string.player_back), onBack, tonal = false)
            }
        }
    }
}

@Composable
private fun EndButton(icon: ImageVector, label: String, onClick: () -> Unit, tonal: Boolean) {
    val content: @Composable () -> Unit = {
        Icon(icon, contentDescription = null, modifier = Modifier.size(20.dp))
        Text(label, Modifier.padding(start = 8.dp), maxLines = 1)
    }
    val m = Modifier.heightIn(min = BIG_BUTTON_DP)
    if (tonal) {
        FilledTonalButton(onClick = onClick, modifier = m, contentPadding = ButtonDefaults.ButtonWithIconContentPadding) { content() }
    } else {
        OutlinedButton(onClick = onClick, modifier = m, contentPadding = ButtonDefaults.ButtonWithIconContentPadding) { content() }
    }
}

/** The auto-next countdown ring (shared by the end screen and the compact [NextEpisodeCard]). */
@Composable
internal fun CountdownRing(secondsLeft: Int, totalSecs: Int, modifier: Modifier = Modifier, size: androidx.compose.ui.unit.Dp = 44.dp) {
    Box(modifier, contentAlignment = Alignment.Center) {
        CircularProgressIndicator(progress = { secondsLeft / totalSecs.coerceAtLeast(1).toFloat() }, modifier = Modifier.size(size))
        Text("$secondsLeft", style = if (size > 48.dp) MaterialTheme.typography.titleMedium else MaterialTheme.typography.labelLarge, fontWeight = FontWeight.Bold)
    }
}

/** From this width the header + actions and the next card sit side by side (landscape phones are ~700-900 dp wide). */
private val WIDE_DP = 600.dp

/** Below this height (a phone in landscape) the end screen uses its compact layout. */
private val SHORT_DP = 480.dp

/** Big touch targets (M3 medium button height). */
private val BIG_BUTTON_DP = 56.dp
