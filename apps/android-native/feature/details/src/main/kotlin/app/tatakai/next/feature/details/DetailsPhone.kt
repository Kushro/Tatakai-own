// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.KeyboardArrowDown
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Replay
import androidx.compose.material.icons.outlined.Cast
import androidx.compose.material.icons.outlined.Download
import androidx.compose.material.icons.outlined.EventAvailable
import androidx.compose.material.icons.outlined.Movie
import androidx.compose.material.icons.outlined.NoAdultContent
import androidx.compose.material.icons.outlined.Share
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.material3.LocalContentColor
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.layout
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.Pill
import app.tatakai.next.designsystem.components.PillTone
import app.tatakai.next.feature.player.sources.sourceSummaryLine
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage

// Ficha "Primero el episodio" (phone, < 840 dp): a compact header (168 dp fanart, the poster overlapping it, the title and, in the A / C
// compositions, the facts line); the rest of the header is the "Cabecera de la ficha" composition (DetailsHead.kt), so the button, the
// tabs and the episode to continue are on screen at scroll 0. Studios, genres and the synopsis live in "Información".

/** Compact phone header: fanart, overlapping poster, title (2 lines) and [facts] (the A / C facts line; null = the title only). */
@Composable
internal fun CompactHeader(
    media: Media,
    state: DetailsUiState,
    actions: DetailsActions,
    facts: (@Composable () -> Unit)? = null,
    /** B / D (no facts line): the "+18" badge goes under the title. */
    adultBadge: Boolean = false,
) {
    val background = MaterialTheme.colorScheme.background
    val title = media.displayTitle(state.titleLanguage)
    val blurred = state.coverBlurred
    val blurModifier = if (blurred) Modifier.blur(24.dp).clickable(onClick = actions.onRevealCover) else Modifier
    val backdrop = headerBackdrop(media, state.artwork)
    Column(Modifier.fillMaxWidth()) {
        Box(Modifier.fillMaxWidth().height(COMPACT_FANART)) {
            AsyncImage(
                model = backdrop.url,
                contentDescription = null,
                contentScale = ContentScale.Crop,
                alignment = if (backdrop.lowRes && media.banner != null) Alignment.BottomCenter else Alignment.Center,
                modifier = Modifier.fillMaxSize().background(MaterialTheme.colorScheme.surfaceContainerHighest).then(blurModifier),
            )
            Box(
                Modifier.fillMaxSize().background(
                    Brush.verticalGradient(
                        listOf(background.copy(alpha = if (backdrop.lowRes) 0.35f else 0.1f), background.copy(alpha = if (backdrop.lowRes) 0.7f else 0.45f), background),
                    ),
                ),
            )
            if (blurred) MatureReveal(actions, Modifier.align(Alignment.Center))
        }
        Row(
            Modifier.fillMaxWidth().padding(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(16.dp),
            verticalAlignment = Alignment.Bottom,
        ) {
            MediaCover(url = media.cover, modifier = Modifier.width(96.dp).overlapUp(POSTER_OVERLAP).then(blurModifier))
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text(
                    title,
                    style = MaterialTheme.typography.headlineSmall,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.semantics { heading() },
                )
                facts?.invoke()
                if (adultBadge) AdultBadge()
            }
        }
    }
}

/** "+18" next to the title when no facts line carries it (B / D). */
@Composable
internal fun AdultBadge(modifier: Modifier = Modifier) {
    Pill(stringResource(R.string.details_adult_badge), tone = PillTone.ERR, modifier = modifier)
}

/** matureContent BLUR: "Toca para mostrar" over the blurred art. */
@Composable
internal fun MatureReveal(actions: DetailsActions, modifier: Modifier = Modifier) {
    Row(
        modifier.heightIn(min = 48.dp).clickable(onClick = actions.onRevealCover).padding(12.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Outlined.NoAdultContent, contentDescription = null, tint = MaterialTheme.colorScheme.onSurface)
        Text(stringResource(R.string.details_mature_tap), style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurface)
    }
}

/** Draws the content [by] higher than its slot and reports that much less height (the poster overlapping the fanart). */
private fun Modifier.overlapUp(by: Dp): Modifier = layout { measurable, constraints ->
    val placeable = measurable.measure(constraints)
    val px = by.roundToPx().coerceAtMost(placeable.height)
    layout(placeable.width, placeable.height - px) { placeable.place(0, -px) }
}

/** Primary button label: [word] may ellipsize, [number] (" · ep. 12") never does; [long] is the TalkBack form. */
internal data class WatchLabels(
    val word: String,
    val number: String?,
    val long: String,
    /** The next-episode card's short label and its spoken form ("Empezar" / "Empezar episodio 1"). */
    val card: String = word,
    val cardLong: String = long,
)

@Composable
internal fun watchLabels(state: DetailsUiState): WatchLabels {
    val ep = state.continueEpisode
    val number = stringResource(R.string.details_ep_suffix, ep)
    return when (state.watchAction) {
        WatchAction.START -> WatchLabels(
            stringResource(R.string.details_watch_short),
            number,
            stringResource(R.string.details_watch_ep, ep),
            card = stringResource(R.string.details_start),
            cardLong = stringResource(R.string.details_start_long, ep),
        )
        WatchAction.CONTINUE -> WatchLabels(stringResource(R.string.details_continue_short), number, stringResource(R.string.details_continue, ep))
        WatchAction.REWATCH -> WatchLabels(stringResource(R.string.details_rewatch), null, stringResource(R.string.details_rewatch_long))
        // nothing watched yet and episode 1 still to air: "Próximamente", not "Al día"
        WatchAction.UP_TO_DATE -> if (state.hasProgress) {
            WatchLabels(stringResource(R.string.details_up_to_date), null, stringResource(R.string.details_up_to_date_long, ep))
        } else {
            WatchLabels(stringResource(R.string.details_coming_soon), null, stringResource(R.string.details_coming_soon_long, ep))
        }
    }
}

/**
 * Split Watch ▾: the leading part plays, the ▾ opens the source picker (picking plays). The label is never ellipsized: the longest form
 * that fits ("Continuar episodio 4", then "Continuar · ep. 4"; [shortLabel] = the card's "Continuar") is measured against the width,
 * and when none fits it wraps to two lines. [sub] goes under the label and [progress] draws a 3 dp bar along the bottom (A).
 */
@Composable
internal fun WatchButton(
    state: DetailsUiState,
    actions: DetailsActions,
    modifier: Modifier = Modifier,
    height: Dp = ACTION_HEIGHT,
    shortLabel: Boolean = false,
    sub: String? = null,
    progress: Float? = null,
    chevronWidth: Dp = 52.dp,
) {
    val action = state.watchAction
    val labels = watchLabels(state)
    val playable = state.playable
    val radius = height / 2
    val leading = RoundedCornerShape(topStart = radius, bottomStart = radius, topEnd = 6.dp, bottomEnd = 6.dp)
    val style = MaterialTheme.typography.titleMedium
    val candidates = when {
        shortLabel -> listOf(labels.card)
        labels.number != null -> listOf(labels.long, labels.word + labels.number)
        else -> listOf(labels.word)
    }
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val bar = MaterialTheme.colorScheme.onPrimary.copy(alpha = 0.55f)
    BoxWithConstraints(modifier) {
        // the leading button's text room: minus the ▾, the 2 dp gap, 2 x 12 dp padding and the 20 + 8 dp icon
        val room = with(density) { (maxWidth - chevronWidth - 2.dp - 24.dp - 28.dp).toPx() }
        val label = remember(candidates, style, room) {
            firstFitting(candidates) { measurer.measure(it, style, maxLines = 1, softWrap = false).size.width <= room } ?: candidates.last()
        }
        val wraps = remember(label, style, room) { measurer.measure(label, style, maxLines = 1, softWrap = false).size.width > room }
        // the spoken name always contains the visible label (WCAG 2.5.3): the long form when it does, else the label itself
        val description = listOfNotNull(labelInName(label, if (shortLabel) labels.cardLong else labels.long), sub).joinToString(", ")
        Row(
            Modifier.fillMaxWidth().heightIn(min = height).height(IntrinsicSize.Min),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            Button(
                onClick = actions.onWatch,
                enabled = playable,
                shape = leading,
                contentPadding = PaddingValues(horizontal = 12.dp),
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
                    .clip(leading)
                    .drawWithContent {
                        drawContent()
                        if (progress != null && playable) {
                            val h = 3.dp.toPx()
                            drawRect(bar, Offset(0f, size.height - h), Size(size.width * progress.coerceIn(0f, 1f), h))
                        }
                    }
                    .semantics { contentDescription = description },
            ) {
                Icon(
                    when (action) {
                        WatchAction.REWATCH -> Icons.Filled.Replay
                        WatchAction.UP_TO_DATE -> Icons.Outlined.EventAvailable
                        else -> Icons.Filled.PlayArrow
                    },
                    contentDescription = null,
                    modifier = Modifier.padding(end = 8.dp).size(20.dp),
                )
                Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(3.dp)) {
                    Text(
                        label,
                        style = style,
                        maxLines = if (wraps) 2 else 1,
                        softWrap = wraps,
                        textAlign = TextAlign.Center,
                    )
                    sub?.let {
                        // wraps at a large font instead of running out of the button
                        Text(
                            it,
                            style = MaterialTheme.typography.labelMedium,
                            color = LocalContentColor.current.copy(alpha = 0.78f),
                            maxLines = 2,
                            textAlign = TextAlign.Center,
                        )
                    }
                }
            }
            Button(
                onClick = { actions.onOpenPicker(true) },
                enabled = playable && state.panel != null && state.hasExtensions,
                shape = RoundedCornerShape(topStart = 6.dp, bottomStart = 6.dp, topEnd = radius, bottomEnd = radius),
                contentPadding = PaddingValues(0.dp),
                modifier = Modifier.width(chevronWidth).fillMaxHeight(),
            ) {
                Icon(Icons.Filled.KeyboardArrowDown, contentDescription = stringResource(R.string.details_choose_source))
            }
        }
    }
}

/** "Más": Enviar a dispositivo, Descargar episodio N / temporada, Compartir; [anchor] draws the control that opens it. */
@Composable
internal fun MoreActionsButton(media: Media, state: DetailsUiState, actions: DetailsActions, anchor: @Composable (open: () -> Unit) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        anchor { open = true }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(
                text = { Text(stringResource(if (state.sending) R.string.details_send_sending else R.string.details_send_device)) },
                onClick = {
                    open = false
                    actions.onSendToDevice()
                },
                enabled = !state.sending && state.playable,
                leadingIcon = { Icon(Icons.Outlined.Cast, contentDescription = null) },
            )
            val panel = state.panel
            val best = state.bestTorrent
            if (panel != null && best != null) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.details_download_episode, panel.episode)) },
                    onClick = {
                        open = false
                        actions.onDownloadSource(best, panel.episode)
                    },
                    enabled = state.playable,
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
                    leadingIcon = { Icon(Icons.Outlined.Download, contentDescription = null) },
                )
            }
            if (shareLink(media) != null) {
                DropdownMenuItem(
                    text = { Text(stringResource(R.string.details_share)) },
                    onClick = {
                        open = false
                        actions.onShare(media)
                    },
                    leadingIcon = { Icon(Icons.Outlined.Share, contentDescription = null) },
                )
            }
        }
    }
}

/**
 * The source as one line (replaces the phone's source card): `● Toko · anizone · 1080p · Subs ES   Cambiar (n)`. Same states as
 * [SourcePanelCard]: no extension, searching, error with retry, nothing found, and the chosen source.
 */
@Composable
internal fun SourceLine(
    state: DetailsUiState,
    actions: DetailsActions,
    modifier: Modifier = Modifier,
    /** Inside the next-episode card: no side margins (the card pads). */
    inCard: Boolean = false,
    /** B: "Ep. 4 · …" before the source. */
    withEpisode: Boolean = false,
) {
    val scheme = MaterialTheme.colorScheme
    val panel = state.panel
    val sources = panel?.sources
    val shown = state.shownSource
    val ok = state.playable && state.hasExtensions && sources is LoadState.Ok && shown != null
    val changeLabel = stringResource(R.string.details_src_change_action)
    val rowModifier = if (ok) Modifier.clickable(onClickLabel = changeLabel) { actions.onOpenPicker(false) } else Modifier
    Row(
        modifier.fillMaxWidth().heightIn(min = 48.dp).then(rowModifier).padding(start = if (inCard) 0.dp else 16.dp, end = if (inCard) 0.dp else if (ok) 16.dp else 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        @Composable
        fun Dot(color: Color) = Box(Modifier.size(8.dp).background(color, CircleShape))

        @Composable
        fun Line(text: String, mod: Modifier = Modifier) = Text(
            text,
            style = MaterialTheme.typography.bodyMedium,
            color = scheme.onSurfaceVariant,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = mod,
        )
        when {
            // "Al día" / "Próximamente": the episode to continue has not aired, there is no source to show or change
            !state.playable -> {
                Dot(scheme.outline)
                Line(stringResource(R.string.details_src_not_aired, state.continueEpisode), Modifier.weight(1f))
            }
            !state.hasExtensions -> {
                Dot(scheme.outline)
                Line(stringResource(R.string.details_no_extension_title), Modifier.weight(1f))
                TextButton(onClick = actions.onOpenExtensions) { Text(stringResource(R.string.details_go_extensions), maxLines = 1) }
            }
            panel == null || sources == null || sources is LoadState.Loading -> {
                CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp)
                Line(stringResource(R.string.details_src_searching), Modifier.weight(1f))
            }
            sources is LoadState.Err -> {
                Dot(scheme.error)
                Line(sources.cause.userMessage(), Modifier.weight(1f))
                TextButton(onClick = actions.onRetrySources) { Text(stringResource(UiR.string.ui_retry), maxLines = 1) }
            }
            shown == null -> {
                Dot(scheme.outline)
                Line(stringResource(R.string.details_no_sources_title), Modifier.weight(1f))
                TextButton(onClick = actions.onOpenExtensions) { Text(stringResource(R.string.details_open_extensions), maxLines = 1) }
            }
            else -> {
                val count = (sources as LoadState.Ok).value.size
                val summary = sourceSummaryLine(shown).let { if (withEpisode) stringResource(R.string.details_src_ep, panel.episode, it) else it }
                val description = stringResource(R.string.details_src_line, summary)
                Dot(scheme.primary)
                Line(summary, Modifier.weight(1f).semantics { contentDescription = description })
                if (panel.loadingMore) CircularProgressIndicator(Modifier.size(14.dp), strokeWidth = 2.dp)
                // never cut: no weight, no wrap
                Text(
                    stringResource(R.string.details_src_change, count),
                    style = MaterialTheme.typography.labelLarge,
                    color = scheme.primary,
                    maxLines = 1,
                    softWrap = false,
                )
            }
        }
    }
}

internal val ACTION_HEIGHT = 52.dp
private val COMPACT_FANART = 168.dp
private val POSTER_OVERLAP = 56.dp
