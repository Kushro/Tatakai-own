// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalLayoutApi::class)

package app.tatakai.next.feature.details

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.TextAutoSize
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.Favorite
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Star
import androidx.compose.material.icons.outlined.BookmarkAdd
import androidx.compose.material.icons.outlined.FavoriteBorder
import androidx.compose.material.icons.outlined.NotificationAdd
import androidx.compose.material.icons.outlined.NotificationsActive
import androidx.compose.material.icons.outlined.Schedule
import androidx.compose.material.icons.outlined.Theaters
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.IconToggleButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.VerticalDivider
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.isTraversalGroup
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import app.tatakai.next.feature.home.rememberFollowControl
import app.tatakai.next.feature.library.ListButton
import app.tatakai.next.model.HeaderLayout
import app.tatakai.next.model.Media
import app.tatakai.next.ui.format.formatDuration
import app.tatakai.next.ui.media.MediaCover
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.systemBars
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import app.tatakai.next.designsystem.components.LocalAppSnackbar
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.ui.R as UiR
import app.tatakai.next.ui.state.rememberIsOnline
import org.koin.compose.koinInject

// "Cabecera de la ficha" (LookPrefs.headerLayout, desktop `header_layout`): four compositions of the same facts and actions under the
// Details header. A = facts line + one watch block, B = labelled facts + named actions, C = facts line + icon tools + the next-episode
// card, D = B's facts and actions + C's card (the default). The main label is never ellipsized and, on a 412 x 800 dp phone, the episode
// to continue is on screen at scroll 0 (the window height picks [HeadFit]: the card drops its cover and times first).

/** Where the next-episode card goes: under the facts (phones), beside them (600-839 dp, D) or in the side column (>= 840 dp). */
internal enum class CardPlace { STACKED, BESIDE, SIDE_COLUMN }

/** Everything the header compositions read, built once per state change in DetailsBody. */
internal data class HeadModel(
    val layout: HeaderLayout,
    val nextUp: NextUp,
    val facts: List<HeadFact>,
    val fit: HeadFit,
    val editorial: Boolean,
    val place: CardPlace,
)

@Composable
internal fun rememberHeadModel(media: Media, state: DetailsUiState, layout: HeaderLayout, editorial: Boolean, place: CardPlace): HeadModel {
    val nowSec = state.clockSec ?: (System.currentTimeMillis() / 1000)
    // the window minus the status and navigation bars (what the list really gets)
    val density = LocalDensity.current
    val bars = WindowInsets.systemBars
    val windowPx = LocalWindowInfo.current.containerSize.height
    val height = with(density) { (windowPx - bars.getTop(density) - bars.getBottom(density)).toDp().value.toInt() }
    val fontScale = density.fontScale
    val total = state.totalEpisodes
    val watched = remember(state.progress, state.listProgress, total) { watchedCount(state.progress, state.listProgress, total) }
    val details = state.detailsOrNull
    val facts = remember(media, details, watched, total, nowSec) { headFacts(media, details, watched, total, nowSec) }
    val next = remember(state.media, state.episodes, state.progress, state.listEntry, state.details, nowSec) { state.nextUp(nowSec) }
    return HeadModel(layout, next, facts, headFit(height, fontScale), editorial, place)
}

/** The facts line for the A / C headers (inside the editorial hero or beside the poster); null for B / D (they use the grid). */
internal fun headerFactsSlot(head: HeadModel, media: Media, state: DetailsUiState): (@Composable () -> Unit)? =
    if (head.layout.usesFacts) {
        null
    } else {
        { FactsLine(head.facts, adult = state.detailsOrNull?.isAdult == true || media.looksAdult(), nextLast = head.layout == HeaderLayout.NEXT_UP) }
    }

/** The editorial hero is 0.4 of the height; B / D give it up to 0.36 on shorter phones so the grid and the card fit. */
internal fun HeadModel.heroFraction(): Float = if (layout.usesFacts && fit != HeadFit.FULL) 0.36f else 0.4f

// ---------------------------------------------------------------- body

/** The part of the header under the title: the composition of [HeadModel.layout] and "Enviando…" while a Link session starts. */
@Composable
internal fun HeaderBody(head: HeadModel, media: Media, state: DetailsUiState, actions: DetailsActions, modifier: Modifier = Modifier) {
    val pad = Modifier.padding(horizontal = 16.dp)
    val side = head.place == CardPlace.SIDE_COLUMN
    // >= 840 dp the source panel is beside the list: A / B keep a reading-width button and no source line
    val buttonWidth = if (side) Modifier.widthIn(max = 420.dp) else Modifier
    val next = head.nextUp
    val sub = next.progress?.let { resumeSub(it) }
    Column(modifier.fillMaxWidth().padding(top = 8.dp, bottom = 4.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
        when (head.layout) {
            HeaderLayout.LINE -> {
                WatchButton(state, actions, pad.then(buttonWidth), height = 56.dp, sub = sub, progress = next.progress?.fraction)
                if (!side) SourceLine(state, actions)
                HeadActions(media, state, actions, HeadActionStyle.BOXED_ICONS)
            }
            HeaderLayout.FACTS -> {
                FactsGrid(head, pad)
                WatchButton(state, actions, pad.then(buttonWidth), height = 56.dp, sub = sub, progress = next.progress?.fraction)
                HeadActions(media, state, actions, HeadActionStyle.CHIPS)
                if (!side) SourceLine(state, actions, withEpisode = true)
            }
            HeaderLayout.NEXT_UP -> {
                // 600-839 dp: the icons span the same reading width as the card
                HeadActions(
                    media, state, actions, HeadActionStyle.PLAIN_ICONS,
                    modifier = if (head.place == CardPlace.BESIDE) Modifier.widthIn(max = 552.dp) else Modifier,
                )
                if (!side) {
                    val width = if (head.place == CardPlace.BESIDE) Modifier.widthIn(max = 520.dp) else Modifier.fillMaxWidth()
                    NextUpCard(head, media, state, actions, footer = true, modifier = pad.then(width))
                }
            }
            HeaderLayout.FACTS_NEXT_UP -> if (head.place == CardPlace.BESIDE) {
                Row(pad.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                    Column(Modifier.weight(1f).semantics { isTraversalGroup = true }, verticalArrangement = Arrangement.spacedBy(12.dp)) {
                        FactsGrid(head)
                        HeadActions(media, state, actions, HeadActionStyle.CHIPS, edge = 0.dp)
                    }
                    NextUpCard(head, media, state, actions, footer = true, modifier = Modifier.width(340.dp).semantics { isTraversalGroup = true })
                }
            } else {
                FactsGrid(head, pad)
                HeadActions(media, state, actions, HeadActionStyle.CHIPS)
                if (!side) NextUpCard(head, media, state, actions, footer = true, modifier = pad.fillMaxWidth())
            }
        }
        // BUG-023: a visible "Enviando…" while the sources resolve and the PC opens the session.
        if (state.sending) {
            Row(
                pad.heightIn(min = 32.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                CircularProgressIndicator(Modifier.size(16.dp), strokeWidth = 2.dp)
                Text(
                    stringResource(R.string.details_send_sending),
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
    }
}

// ---------------------------------------------------------------- facts

/** "2 d 5 h" / "5 h 12 min" / "12 min" until the next episode. */
@Composable
internal fun whenText(left: Countdown): String = when {
    left.days > 0 -> stringResource(R.string.details_when_days, left.days, left.hours)
    left.hours > 0 -> stringResource(R.string.details_when_hours, left.hours, left.minutes)
    else -> stringResource(R.string.details_when_minutes, maxOf(1L, left.minutes))
}

/** The countdown as TalkBack says it: "2 días y 5 horas", "5 horas y 12 minutos", "12 minutos". */
@Composable
internal fun spokenWhen(left: Countdown): String {
    @Composable
    fun q(res: Int, n: Long) = pluralStringResource(res, n.toInt(), n.toInt())
    return when {
        left.days > 0 -> if (left.hours > 0) {
            stringResource(R.string.details_and, q(R.plurals.details_days_spoken, left.days), q(R.plurals.details_hours_spoken, left.hours))
        } else {
            q(R.plurals.details_days_spoken, left.days)
        }
        left.hours > 0 -> if (left.minutes > 0) {
            stringResource(R.string.details_and, q(R.plurals.details_hours_spoken, left.hours), q(R.plurals.details_minutes_spoken, left.minutes))
        } else {
            q(R.plurals.details_hours_spoken, left.hours)
        }
        else -> q(R.plurals.details_minutes_spoken, maxOf(1L, left.minutes))
    }
}

/** A / B: "3:39 de 24 min" (or "de ~24 min" from the runtime) under the main label; "Retomar en 3:39" without a length. */
@Composable
internal fun resumeSub(p: ResumeProgress): String {
    val at = formatDuration(p.watchedSec.toLong())
    val min = p.durationMin
    return when {
        min == null -> stringResource(R.string.details_resume_at, at)
        p.estimated -> stringResource(R.string.details_progress_of_approx, at, min)
        else -> stringResource(R.string.details_progress_of, at, min)
    }
}

/**
 * The facts as one quiet line (A, C): items never wrap inside and never ellipsize, the line wraps between them. A joins the status and
 * the next episode ("En emisión · ep. 9 en 2 d 5 h"); C ([nextLast]) moves the next episode to the end with a clock. One TalkBack node.
 */
@Composable
internal fun FactsLine(facts: List<HeadFact>, adult: Boolean, nextLast: Boolean, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val status = facts.filterIsInstance<HeadFact.Status>().firstOrNull()
    val next = facts.filterIsInstance<HeadFact.Next>().firstOrNull()
    val statusLabel = status?.let { statusPill(it.raw)?.first }
    val nextWhen = next?.let { whenText(it.left) }
    val nextSpoken = next?.let { stringResource(R.string.details_next_short_cd, it.episode, spokenWhen(it.left)) }
    val spoken = mutableListOf<String>()
    val items = mutableListOf<@Composable () -> Unit>()
    @Composable
    fun plain(text: String, color: Color = scheme.onSurfaceVariant, lead: (@Composable () -> Unit)? = null) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            lead?.invoke()
            Text(text, style = MaterialTheme.typography.bodyMedium, color = color, maxLines = 1, softWrap = false)
        }
    }
    val live = status?.raw == "RELEASING"
    if (statusLabel != null) {
        val text = if (!nextLast && next != null && nextWhen != null) {
            stringResource(R.string.details_line_status_next, statusLabel, next.episode, nextWhen)
        } else {
            statusLabel
        }
        spoken += if (!nextLast && nextSpoken != null) "$statusLabel, $nextSpoken" else statusLabel
        items += { plain(text, if (live) scheme.onSurface else scheme.onSurfaceVariant, if (live) ({ LiveDot() }) else null) }
    } else if (!nextLast && next != null && nextWhen != null) {
        val text = stringResource(R.string.details_next_short, next.episode, nextWhen)
        spoken += nextSpoken ?: text
        items += { plain(text) }
    }
    facts.forEach { fact ->
        when (fact) {
            is HeadFact.Year -> fact.year.toString().let { t -> spoken += t; items += { plain(t) } }
            is HeadFact.Episodes -> {
                val t = if (fact.total != null) {
                    stringResource(R.string.details_eps_short, fact.total)
                } else {
                    "${fact.watched} ${pluralStringResource(R.plurals.details_fact_watched, fact.watched)}"
                }
                spoken += t
                items += { plain(t) }
            }
            is HeadFact.Score -> {
                val score = formatScore(fact.score)
                spoken += stringResource(R.string.details_score, score)
                items += { plain(score, lead = { Icon(Icons.Filled.Star, null, tint = scheme.primary, modifier = Modifier.size(14.dp)) }) }
            }
            else -> Unit
        }
    }
    if (adult) stringResource(R.string.details_adult_badge).let { t -> spoken += t; items += { plain(t) } }
    if (nextLast && next != null && nextWhen != null) {
        val t = stringResource(R.string.details_next_short, next.episode, nextWhen)
        spoken += nextSpoken ?: t
        items += { plain(t, lead = { Icon(Icons.Outlined.Schedule, null, tint = scheme.primary, modifier = Modifier.size(14.dp)) }) }
    }
    if (items.isEmpty()) return
    val description = spoken.joinToString(", ")
    FlowRow(
        modifier.clearAndSetSemantics { contentDescription = description },
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
        itemVerticalAlignment = Alignment.CenterVertically,
    ) {
        items.forEach { it() }
    }
}

@Composable
private fun LiveDot() = Box(Modifier.size(7.dp).background(MaterialTheme.colorScheme.tertiary, CircleShape))

/**
 * Labelled facts (B, D): rows of 3 (2 at 130 % font or when a cell would be under 96 dp), a hairline before every cell but the first
 * of its row, drawn the row's full height. [gridFacts] / [gridMaxRows] decide what is left out when the rows are capped.
 */
@Composable
internal fun FactsGrid(head: HeadModel, modifier: Modifier = Modifier) {
    val fontScale = LocalDensity.current.fontScale
    val line = MaterialTheme.colorScheme.outline.copy(alpha = 0.55f)
    BoxWithConstraints(modifier.fillMaxWidth()) {
        val columns = gridColumns(maxWidth.value, fontScale)
        val shown = gridFacts(head.facts, columns, gridMaxRows(head.fit, fontScale))
        if (shown.isEmpty()) return@BoxWithConstraints
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            shown.chunked(columns).forEach { row ->
                Row(
                    Modifier.fillMaxWidth().drawBehind {
                        val cell = size.width / columns
                        for (i in 1 until row.size) {
                            drawLine(line, Offset(cell * i, 0f), Offset(cell * i, size.height), strokeWidth = 1.dp.toPx())
                        }
                    },
                ) {
                    row.forEachIndexed { i, fact -> FactCell(fact, first = i == 0, Modifier.weight(1f)) }
                    repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
    }
}

@Composable
private fun FactCell(fact: HeadFact, first: Boolean, modifier: Modifier) {
    val scheme = MaterialTheme.colorScheme
    val label: String
    val value: String
    var note: String? = null
    var lead: (@Composable () -> Unit)? = null
    val spoken: String
    when (fact) {
        is HeadFact.Status -> {
            label = stringResource(R.string.details_info_status)
            value = statusPill(fact.raw)?.first ?: fact.raw
            if (fact.raw == "RELEASING") lead = { LiveDot() }
            spoken = stringResource(R.string.details_fact_cd, label, value)
        }
        is HeadFact.Next -> {
            label = stringResource(R.string.details_fact_next)
            value = stringResource(R.string.details_fact_next_value, fact.episode)
            note = whenText(fact.left)
            spoken = stringResource(R.string.details_fact_cd, label, stringResource(R.string.details_next_short_cd, fact.episode, spokenWhen(fact.left)))
        }
        is HeadFact.Episodes -> {
            label = stringResource(R.string.details_episodes)
            when {
                fact.total != null && fact.watched > 0 -> {
                    value = fact.watched.toString()
                    note = stringResource(R.string.details_fact_of_total, fact.total)
                    spoken = stringResource(R.string.details_fact_episodes_cd, fact.watched, fact.total)
                }
                fact.total != null -> {
                    value = fact.total.toString()
                    spoken = stringResource(R.string.details_fact_episodes_total_cd, fact.total)
                }
                else -> {
                    value = fact.watched.toString()
                    note = pluralStringResource(R.plurals.details_fact_watched, fact.watched)
                    spoken = stringResource(R.string.details_fact_cd, label, "$value $note")
                }
            }
        }
        is HeadFact.Year -> {
            label = stringResource(R.string.details_fact_year)
            value = fact.year.toString()
            spoken = stringResource(R.string.details_fact_cd, label, value)
        }
        is HeadFact.Score -> {
            label = stringResource(R.string.details_fact_score)
            value = formatScore(fact.score)
            lead = { Icon(Icons.Filled.Star, null, tint = scheme.primary, modifier = Modifier.size(15.dp)) }
            spoken = stringResource(R.string.details_fact_cd, label, value)
        }
    }
    Column(
        modifier
            .padding(start = if (first) 0.dp else 10.dp, end = 10.dp)
            .clearAndSetSemantics { contentDescription = spoken },
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(
            label.uppercase(),
            style = MaterialTheme.typography.labelSmall.copy(letterSpacing = 0.8.sp),
            color = scheme.onSurfaceVariant,
            maxLines = 1,
            softWrap = false,
        )
        // the dot keeps its size and the value takes the rest, shrinking a long status ("Próximamente") instead of cutting it
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
            lead?.invoke()
            Text(
                value,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                softWrap = false,
                autoSize = TextAutoSize.StepBased(minFontSize = 12.sp, maxFontSize = MaterialTheme.typography.titleMedium.fontSize),
                modifier = Modifier.weight(1f, fill = false),
            )
        }
        // the note on its own line, so it never pushes the value out of the cell
        note?.let {
            Text(
                it,
                style = MaterialTheme.typography.bodySmall,
                color = scheme.onSurfaceVariant,
                maxLines = 1,
                softWrap = false,
                autoSize = TextAutoSize.StepBased(minFontSize = 10.sp, maxFontSize = MaterialTheme.typography.bodySmall.fontSize),
            )
        }
    }
}

// ---------------------------------------------------------------- actions

internal enum class HeadActionStyle { BOXED_ICONS, PLAIN_ICONS, CHIPS }

/**
 * The title's tools in a fixed order: Lista, Favorito, Seguir, Tráiler, Más. Icons in an even row (A boxed, C plain) or named chips
 * that slide sideways to the screen edge (B, D). Favorito, Seguir and Tráiler are left out when they do not apply.
 */
@Composable
internal fun HeadActions(
    media: Media,
    state: DetailsUiState,
    actions: DetailsActions,
    style: HeadActionStyle,
    modifier: Modifier = Modifier,
    /** Chips: the start / end inset of the sliding row (the screen margin). */
    edge: Dp = 16.dp,
) {
    val scheme = MaterialTheme.colorScheme
    val listWord = stringResource(R.string.details_action_list)
    val lists = koinInject<ListsRepository>()
    val accounts by lists.accounts.collectAsState()
    val online = rememberIsOnline()
    val snackbar = LocalAppSnackbar.current
    val queuedOffline = stringResource(UiR.string.list_queued_offline)
    val follow = rememberFollowControl(media)
    val trailer = state.detailsOrNull?.trailer
    val content: @Composable () -> Unit = {
        ListButton(
            media = media,
            anchor = { label, entry, open ->
                HeadAction(
                    style,
                    icon = if (entry != null) Icons.Filled.Bookmark else Icons.Outlined.BookmarkAdd,
                    label = if (entry != null) label else listWord,
                    on = entry != null,
                    onClick = open,
                    // "Lista: Viendo" / "Lista: Añadir a lista" (the visible word first)
                    description = stringResource(R.string.details_fact_cd, listWord, label),
                )
            },
        )
        actions.onToggleFavorite?.let { toggle ->
            val fav = state.favorite
            HeadAction(
                style,
                icon = if (fav) Icons.Filled.Favorite else Icons.Outlined.FavoriteBorder,
                label = stringResource(R.string.details_action_favorite),
                on = fav,
                checked = fav,
                onClick = {
                    toggle()
                    // like the list menu: a linked account offline syncs later
                    if (accounts.isNotEmpty() && !online) snackbar.show(queuedOffline)
                },
            )
        }
        follow?.let { f ->
            HeadAction(
                style,
                icon = if (f.following) Icons.Outlined.NotificationsActive else Icons.Outlined.NotificationAdd,
                label = stringResource(if (f.following) R.string.details_action_following else R.string.details_action_follow),
                on = f.following,
                checked = f.following,
                onClick = f.toggle,
            )
        }
        trailer?.let { t ->
            HeadAction(style, icon = Icons.Outlined.Theaters, label = stringResource(R.string.details_trailer), on = false, onClick = { actions.onOpenUrl(t.url) })
        }
        if (style == HeadActionStyle.BOXED_ICONS) VerticalDivider(Modifier.height(24.dp), color = scheme.outlineVariant)
        MoreActionsButton(media, state, actions) { open ->
            HeadAction(
                style,
                icon = Icons.Filled.MoreVert,
                label = stringResource(R.string.details_action_more),
                on = false,
                onClick = open,
                description = stringResource(R.string.details_more_actions),
            )
        }
    }
    val group = Modifier.semantics { isTraversalGroup = true }
    when (style) {
        HeadActionStyle.CHIPS -> Row(
            modifier.then(group).fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = edge),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) { content() }
        HeadActionStyle.PLAIN_ICONS -> Row(
            modifier.then(group).fillMaxWidth().padding(horizontal = 20.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) { content() }
        HeadActionStyle.BOXED_ICONS -> Row(
            modifier
                .then(group)
                .padding(horizontal = 16.dp)
                .fillMaxWidth()
                .border(1.dp, scheme.outlineVariant, CircleShape)
                .background(scheme.surfaceContainerHigh.copy(alpha = 0.6f), CircleShape)
                .padding(horizontal = 8.dp, vertical = 2.dp),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) { content() }
    }
}

/** One tool: an icon button (48 dp target) or a 40 dp chip; [checked] != null makes it a toggle (Favorito, Seguir). */
@Composable
private fun HeadAction(
    style: HeadActionStyle,
    icon: ImageVector,
    label: String,
    on: Boolean,
    onClick: () -> Unit,
    checked: Boolean? = null,
    description: String = label,
) {
    val scheme = MaterialTheme.colorScheme
    val tint = when {
        on -> scheme.primary
        style == HeadActionStyle.BOXED_ICONS -> scheme.onSurface
        else -> scheme.onSurfaceVariant
    }
    if (style != HeadActionStyle.CHIPS) {
        if (checked != null) {
            IconToggleButton(checked = checked, onCheckedChange = { onClick() }, modifier = Modifier.semantics { role = Role.Switch }) {
                Icon(icon, contentDescription = description, tint = tint)
            }
        } else {
            IconButton(onClick = onClick) { Icon(icon, contentDescription = description, tint = tint) }
        }
        return
    }
    val border = BorderStroke(1.dp, if (on) scheme.primary.copy(alpha = 0.6f) else scheme.outlineVariant)
    val color = if (on) scheme.primary else scheme.onSurface
    val chip: @Composable () -> Unit = {
        Row(
            Modifier.height(40.dp).padding(start = 12.dp, end = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(label, style = MaterialTheme.typography.labelLarge, maxLines = 1, softWrap = false)
        }
    }
    val chipModifier = if (description != label) Modifier.semantics { contentDescription = description } else Modifier
    if (checked != null) {
        Surface(
            checked = checked,
            onCheckedChange = { onClick() },
            shape = CircleShape,
            color = Color.Transparent,
            contentColor = color,
            border = border,
            modifier = chipModifier.semantics { role = Role.Switch },
            content = chip,
        )
    } else {
        Surface(
            onClick = onClick,
            shape = CircleShape,
            color = Color.Transparent,
            contentColor = color,
            border = border,
            modifier = chipModifier,
            content = chip,
        )
    }
}

// ---------------------------------------------------------------- next-episode card

/**
 * C / D: the episode to play with its number, title and resume point, the main button (short label, long form for TalkBack) and, when
 * [footer] and it plays, the source line. [HeadFit.TIGHT] drops the cover and the times row; [HeadFit.SHORT] also the bar (the resume
 * time moves to the first line).
 */
@Composable
internal fun NextUpCard(head: HeadModel, media: Media, state: DetailsUiState, actions: DetailsActions, footer: Boolean, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val next = head.nextUp
    val fit = head.fit
    val progress = next.progress
    val watchedText = progress?.let { formatDuration(it.watchedSec.toLong()) }
    // a length taken from the runtime is an estimate: "quedan ~20 min" instead of an exact "quedan 20:21"
    val remainingText = progress?.let { p ->
        if (p.estimated) p.remainingMin?.let { stringResource(R.string.details_minutes_approx, it) } else p.remainingSec?.let { formatDuration(it.toLong()) }
    }
    val remainingSpoken = progress?.let { p ->
        if (p.estimated) p.remainingMin?.let { stringResource(R.string.details_minutes_approx_cd, it) } else remainingText
    }
    val airingLeft = next.airing?.let { countdown(it.airingAtEpochSec, state.clockSec ?: (System.currentTimeMillis() / 1000)) }
    val airingWhen = airingLeft?.let { whenText(it) }
    val airingSpoken = airingLeft?.let { spokenWhen(it) }
    val format = state.detailsOrNull?.format ?: media.format
    val movieLabel = formatLabel(format ?: "MOVIE")
    val airingEp = next.airing?.episode ?: next.episode
    val kicker = when (next.kind) {
        NextUpKind.REWATCH -> stringResource(if (next.movie) R.string.details_next_movie_watched else R.string.details_next_all_watched)
        NextUpKind.UP_TO_DATE -> airingWhen?.let { stringResource(R.string.details_next_short, airingEp, it) }
            ?: stringResource(R.string.details_next_ep, next.episode)
        NextUpKind.UPCOMING -> listOfNotNull(stringResource(R.string.details_coming_soon), airingWhen).joinToString(" · ")
        else -> when {
            next.movie -> movieLabel
            next.total != null -> stringResource(R.string.details_next_ep_of, next.episode, next.total)
            else -> stringResource(R.string.details_next_ep, next.episode)
        }
    }
    val resumeInKicker = fit == HeadFit.SHORT && watchedText != null
    val kickerShown = if (resumeInKicker) "$kicker · ${stringResource(R.string.details_resume_at, watchedText)}" else kicker
    val title = next.title.takeIf { next.kind != NextUpKind.REWATCH }
    // Spoken once for the whole block, times included even when TIGHT hides them.
    val epSpoken = when (next.kind) {
        NextUpKind.CONTINUE, NextUpKind.START -> when {
            next.movie -> movieLabel
            next.total != null -> stringResource(R.string.details_next_ep_of_cd, next.episode, next.total)
            else -> stringResource(R.string.details_next_ep_cd, next.episode)
        }
        // words, not "2 d 5 h" letter by letter
        NextUpKind.UP_TO_DATE -> airingSpoken?.let { stringResource(R.string.details_next_short_cd, airingEp, it) } ?: kicker
        NextUpKind.UPCOMING -> listOfNotNull(
            stringResource(R.string.details_coming_soon),
            airingSpoken?.let { stringResource(R.string.details_next_short_cd, airingEp, it) },
        ).joinToString(", ")
        NextUpKind.REWATCH -> kicker
    }
    val progressSpoken = when {
        watchedText != null && remainingSpoken != null -> stringResource(R.string.details_next_progress_cd, watchedText, remainingSpoken)
        watchedText != null -> stringResource(R.string.details_resume_at, watchedText)
        else -> null
    }
    val spoken = stringResource(R.string.details_next_region) + ": " + listOfNotNull(epSpoken, title, progressSpoken).joinToString(", ")
    val fontScale = LocalDensity.current.fontScale
    Surface(
        shape = RoundedCornerShape(20.dp),
        color = scheme.surfaceContainerHigh,
        border = BorderStroke(1.dp, scheme.outlineVariant),
        modifier = modifier,
    ) {
        Column(Modifier.padding(14.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                if (head.editorial && fit == HeadFit.FULL && !state.coverBlurred) {
                    MediaCover(url = media.cover, modifier = Modifier.width(48.dp), shape = RoundedCornerShape(8.dp))
                }
                Column(Modifier.weight(1f).clearAndSetSemantics { contentDescription = spoken }) {
                    Text(
                        kickerShown,
                        style = MaterialTheme.typography.labelMedium,
                        color = scheme.primary,
                    )
                    title?.let {
                        Text(
                            it,
                            style = MaterialTheme.typography.titleMedium,
                            maxLines = if (fontScale >= 1.3f) 2 else 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.padding(top = 4.dp),
                        )
                    }
                    val fraction = progress?.fraction
                    if (fraction != null && fit != HeadFit.SHORT) {
                        Box(
                            Modifier
                                .padding(top = 8.dp)
                                .fillMaxWidth()
                                .height(4.dp)
                                .background(scheme.surfaceContainerHighest, CircleShape),
                        ) {
                            Box(Modifier.fillMaxWidth(fraction).fillMaxHeight().background(scheme.primary, CircleShape))
                        }
                        if (fit == HeadFit.FULL && watchedText != null && remainingText != null) {
                            Row(Modifier.fillMaxWidth().padding(top = 5.dp)) {
                                Text(watchedText, style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant, modifier = Modifier.weight(1f))
                                Text(stringResource(R.string.details_next_remaining, remainingText), style = MaterialTheme.typography.labelSmall, color = scheme.onSurfaceVariant)
                            }
                        }
                    } else if (watchedText != null && !resumeInKicker) {
                        Text(
                            stringResource(R.string.details_resume_at, watchedText),
                            style = MaterialTheme.typography.bodySmall,
                            color = scheme.onSurfaceVariant,
                            modifier = Modifier.padding(top = 4.dp),
                        )
                    }
                }
            }
            WatchButton(state, actions, Modifier.fillMaxWidth(), height = 48.dp, shortLabel = true, chevronWidth = 48.dp)
            if (footer && next.playable) {
                Column {
                    HorizontalDivider(color = scheme.outlineVariant)
                    SourceLine(state, actions, inCard = true)
                }
            }
        }
    }
}
