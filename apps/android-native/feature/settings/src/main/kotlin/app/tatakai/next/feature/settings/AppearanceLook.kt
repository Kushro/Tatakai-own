// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.model.CalendarView
import app.tatakai.next.model.CardSize
import app.tatakai.next.model.CardStyle
import app.tatakai.next.model.Density
import app.tatakai.next.model.EpisodeView
import app.tatakai.next.model.HeaderLayout
import app.tatakai.next.model.HomeLayout
import app.tatakai.next.model.LookPrefs
import app.tatakai.next.model.LookPreset
import app.tatakai.next.model.Motion
import app.tatakai.next.model.PageStyle
import app.tatakai.next.model.RailStyle
import app.tatakai.next.model.RelationsView
import kotlin.math.roundToInt

// 2026-11 UI refresh: Settings › Apariencia › Diseño y estilo (desktop settings/appearance.rs). A preset applies every option at
// once; changing one afterwards shows "Personalizado". Palette / mode / AMOLED stay above, as before.
//
// Android adaptation: "Menú lateral" only affects the navigation rail of tablets / wide windows (phones keep the bottom bar, labelled);
// "Efectos de desenfoque" (glass) is not offered: a real-time blur is too heavy for low-end phones, so the field is kept only so the
// presets stay identical to the desktop ones.

@Composable
internal fun LookGroup(look: LookPrefs, onChange: ((LookPrefs) -> LookPrefs) -> Unit, onPreset: (LookPreset) -> Unit) {
    val matching = look.matchingPreset()
    Column(Modifier.fillMaxWidth().padding(top = 16.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
        Text(
            stringResource(R.string.settings_look_title),
            style = MaterialTheme.typography.titleSmall,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.semantics { heading() },
        )
        Text(
            stringResource(R.string.settings_look_hint),
            style = MaterialTheme.typography.bodySmall,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        LazyRow(
            contentPadding = PaddingValues(vertical = 8.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(LookPreset.entries, key = { it.id }, contentType = { "preset" }) { preset ->
                PresetCard(preset, selected = preset == matching, onClick = { onPreset(preset) })
            }
        }
        Text(
            if (matching == null) stringResource(R.string.settings_look_custom) else stringResource(R.string.settings_look_preset_on, stringResource(matching.labelRes())),
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
    LookSegmented(
        R.string.settings_look_home,
        HomeLayout.entries,
        look.homeLayout,
        { it.labelRes() },
        hint = stringResource(R.string.settings_look_home_hint),
    ) { v -> onChange { it.copy(homeLayout = v) } }
    LookSegmented(
        R.string.settings_look_style,
        PageStyle.entries,
        look.pageStyle,
        { it.labelRes() },
        hint = stringResource(R.string.settings_look_style_hint),
    ) { v -> onChange { it.copy(pageStyle = v) } }
    SettingRow(stringResource(R.string.settings_look_header), hint = stringResource(R.string.settings_look_header_hint), stack = true) {
        // 2 x 2, not a sliding row: the default (D) is never off screen, and the cards grow with the font instead of cutting the hint
        Column(Modifier.fillMaxWidth().selectableGroup(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            HeaderLayout.entries.chunked(2).forEach { pair ->
                Row(Modifier.fillMaxWidth().height(IntrinsicSize.Min), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    pair.forEach { layout ->
                        HeaderLayoutCard(
                            layout,
                            selected = layout == look.headerLayout,
                            onClick = { onChange { it.copy(headerLayout = layout) } },
                            modifier = Modifier.weight(1f).fillMaxHeight(),
                        )
                    }
                }
            }
        }
    }
    LookSegmented(R.string.settings_look_cards, CardStyle.entries, look.cardStyle, { it.labelRes() }) { v -> onChange { it.copy(cardStyle = v) } }
    LookSegmented(
        R.string.settings_look_card_size,
        CardSize.entries,
        look.cardSize,
        { it.labelRes() },
        hint = stringResource(R.string.settings_look_card_size_hint),
        small = true,
    ) { v -> onChange { it.copy(cardSize = v) } }
    LookSegmented(R.string.settings_look_density, Density.entries, look.density, { it.labelRes() }) { v -> onChange { it.copy(density = v) } }
    LookSegmented(
        R.string.settings_look_motion,
        Motion.entries,
        look.motion,
        { it.labelRes() },
        hint = stringResource(R.string.settings_look_motion_hint),
    ) { v -> onChange { it.copy(motion = v) } }
    LookSegmented(R.string.settings_look_episodes, EpisodeView.entries, look.episodeView, { it.labelRes() }) { v -> onChange { it.copy(episodeView = v) } }
    LookSegmented(R.string.settings_look_relations, RelationsView.entries, look.relationsView, { it.labelRes() }) { v -> onChange { it.copy(relationsView = v) } }
    LookSegmented(R.string.settings_look_calendar, CalendarView.entries, look.calendarView, { it.labelRes() }, small = true) { v ->
        onChange { it.copy(calendarView = v) }
    }
    LookSegmented(
        R.string.settings_look_rail,
        RailStyle.entries,
        look.railStyle,
        { it.labelRes() },
        hint = stringResource(R.string.settings_look_rail_hint),
    ) { v -> onChange { it.copy(railStyle = v) } }
    if (look.homeLayout != HomeLayout.ROWS) {
        SwitchRow(
            stringResource(R.string.settings_look_autoplay),
            look.heroAutoplay,
            { on -> onChange { it.copy(heroAutoplay = on) } },
            hint = stringResource(R.string.settings_look_autoplay_hint),
        )
        if (look.heroAutoplay) IntervalRow(look.heroIntervalSecs) { secs -> onChange { it.copy(heroIntervalSecs = secs) } }
    }
    SwitchRow(stringResource(R.string.settings_look_greeting), look.showGreeting, { on -> onChange { it.copy(showGreeting = on) } })
}

@Composable
private fun <T> LookSegmented(
    labelRes: Int,
    options: List<T>,
    selected: T,
    optionLabel: (T) -> Int,
    hint: String? = null,
    small: Boolean = false,
    onSelect: (T) -> Unit,
) {
    SettingRow(stringResource(labelRes), hint = hint, stack = true) {
        Segmented(
            options = options.map { SegmentedOption(it, stringResource(optionLabel(it))) },
            selected = selected,
            onSelect = onSelect,
            fillWidth = true,
            small = small,
        )
    }
}

@Composable
private fun IntervalRow(secs: Int, onChange: (Int) -> Unit) {
    // Local while dragging, saved once on release (no DataStore write per frame).
    var drag by remember(secs) { mutableFloatStateOf(secs.toFloat()) }
    val shown = drag.roundToInt()
    SettingRow(stringResource(R.string.settings_look_interval), hint = stringResource(R.string.settings_look_interval_value, shown), stack = true) {
        Slider(
            value = drag,
            onValueChange = { drag = it },
            onValueChangeFinished = { onChange(drag.roundToInt()) },
            valueRange = LookPrefs.HERO_INTERVAL_MIN.toFloat()..LookPrefs.HERO_INTERVAL_MAX.toFloat(),
            steps = LookPrefs.HERO_INTERVAL_MAX - LookPrefs.HERO_INTERVAL_MIN - 1,
            modifier = Modifier.semantics { stateDescription = "$shown s" },
        )
    }
}

@Composable
private fun PresetCard(preset: LookPreset, selected: Boolean, onClick: () -> Unit) {
    Surface(
        onClick = onClick,
        shape = MaterialTheme.shapes.large,
        color = if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        border = if (selected) BorderStroke(2.dp, MaterialTheme.colorScheme.primary) else null,
        modifier = Modifier.width(156.dp).semantics {
            role = Role.RadioButton
            this.selected = selected
        },
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    stringResource(preset.labelRes()),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.weight(1f),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (selected) Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
            }
            Text(
                stringResource(preset.hintRes()),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 3,
                minLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

internal fun LookPreset.labelRes(): Int = when (this) {
    LookPreset.TATAKAI -> R.string.settings_look_preset_tatakai
    LookPreset.CINEMA -> R.string.settings_look_preset_cinema
    LookPreset.DENSE -> R.string.settings_look_preset_dense
    LookPreset.CALM -> R.string.settings_look_preset_calm
}

private fun LookPreset.hintRes(): Int = when (this) {
    LookPreset.TATAKAI -> R.string.settings_look_preset_tatakai_hint
    LookPreset.CINEMA -> R.string.settings_look_preset_cinema_hint
    LookPreset.DENSE -> R.string.settings_look_preset_dense_hint
    LookPreset.CALM -> R.string.settings_look_preset_calm_hint
}

/** One composition of the Details header: a drawn thumbnail, its name and a two-line hint (radio card, like [PresetCard]). */
@Composable
private fun HeaderLayoutCard(layout: HeaderLayout, selected: Boolean, onClick: () -> Unit, modifier: Modifier = Modifier) {
    Surface(
        selected = selected,
        onClick = onClick,
        shape = MaterialTheme.shapes.large,
        color = if (selected) MaterialTheme.colorScheme.secondaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        border = if (selected) BorderStroke(2.dp, MaterialTheme.colorScheme.primary) else null,
        modifier = modifier.semantics { role = Role.RadioButton },
    ) {
        Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            HeaderLayoutThumb(layout)
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Text(
                    stringResource(layout.labelRes()),
                    style = MaterialTheme.typography.titleSmall,
                    modifier = Modifier.weight(1f),
                )
                if (selected) Icon(Icons.Filled.CheckCircle, contentDescription = null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(18.dp))
            }
            Text(
                stringResource(layout.hintRes()),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/**
 * Schematic of [layout] (124:76, as wide as its card): the image band, then a line (A, C) or 3 + 2 fact cells (B, D), the main pill loose (A, B) or
 * inside an outlined card (C, D), and icon dots (A, C) or named tags (B, D). Decorative: the card's text says it.
 */
@Composable
private fun HeaderLayoutThumb(layout: HeaderLayout) {
    val scheme = MaterialTheme.colorScheme
    val band = scheme.surfaceContainerHighest
    val ink = scheme.onSurfaceVariant.copy(alpha = 0.55f)
    val faint = scheme.outlineVariant
    val primary = scheme.primary
    Canvas(
        Modifier
            .fillMaxWidth()
            .aspectRatio(124f / 76f)
            .clip(MaterialTheme.shapes.small)
            .background(scheme.surface)
            .clearAndSetSemantics {},
    ) {
        // drawn in 76 units of height, so the schematic scales with the card
        val u = size.height / 76f
        val pad = 6 * u
        val w = size.width - 2 * pad
        drawRect(band, size = Size(size.width, 22 * u))
        var y = 26 * u
        fun bar(x: Float, top: Float, width: Float, height: Float, color: Color) =
            drawRoundRect(color, Offset(x, top), Size(width, height), CornerRadius(height / 2))
        fun line() {
            bar(pad, y, w * 0.62f, 3 * u, ink)
            y += 7 * u
        }
        fun grid() {
            val cell = w / 3
            for (i in 0 until 5) {
                val col = i % 3
                val row = i / 3
                bar(pad + col * cell, y + row * 7 * u, cell - 4 * u, 4 * u, ink)
            }
            y += 15 * u
        }
        fun pill(left: Float, width: Float) {
            bar(left, y, width, 7 * u, primary)
            y += 11 * u
        }
        fun dots() {
            for (i in 0 until 5) drawCircle(ink, 2.2f * u, Offset(pad + 3 * u + i * (w - 6 * u) / 4, y + 2 * u))
            y += 8 * u
        }
        fun tags() {
            val tag = 18 * u
            for (i in 0 until 4) {
                drawRoundRect(faint, Offset(pad + i * (tag + 4 * u), y), Size(tag, 6 * u), CornerRadius(3 * u), style = Stroke(u))
            }
            y += 10 * u
        }
        fun card() {
            val h = 18 * u
            drawRoundRect(faint, Offset(pad, y), Size(w, h), CornerRadius(4 * u), style = Stroke(u))
            bar(pad + 4 * u, y + 3 * u, w * 0.4f, 3 * u, ink)
            bar(pad + 4 * u, y + 9 * u, w - 8 * u, 6 * u, primary)
            y += h + 4 * u
        }
        when (layout) {
            HeaderLayout.LINE -> { line(); pill(pad, w); dots() }
            HeaderLayout.FACTS -> { grid(); pill(pad, w); tags() }
            HeaderLayout.NEXT_UP -> { line(); dots(); card() }
            HeaderLayout.FACTS_NEXT_UP -> { grid(); tags(); card() }
        }
    }
}

private fun HeaderLayout.labelRes(): Int = when (this) {
    HeaderLayout.LINE -> R.string.settings_look_header_line
    HeaderLayout.FACTS -> R.string.settings_look_header_facts
    HeaderLayout.NEXT_UP -> R.string.settings_look_header_nextup
    HeaderLayout.FACTS_NEXT_UP -> R.string.settings_look_header_facts_nextup
}

private fun HeaderLayout.hintRes(): Int = when (this) {
    HeaderLayout.LINE -> R.string.settings_look_header_line_hint
    HeaderLayout.FACTS -> R.string.settings_look_header_facts_hint
    HeaderLayout.NEXT_UP -> R.string.settings_look_header_nextup_hint
    HeaderLayout.FACTS_NEXT_UP -> R.string.settings_look_header_facts_nextup_hint
}

private fun PageStyle.labelRes(): Int = when (this) {
    PageStyle.STANDARD -> R.string.settings_look_style_standard
    PageStyle.EDITORIAL -> R.string.settings_look_style_editorial
}

private fun HomeLayout.labelRes(): Int = when (this) {
    HomeLayout.SPOTLIGHT -> R.string.settings_look_home_spotlight
    HomeLayout.CAROUSEL -> R.string.settings_look_home_carousel
    HomeLayout.ROWS -> R.string.settings_look_home_rows
}

private fun CardStyle.labelRes(): Int = when (this) {
    CardStyle.CLASSIC -> R.string.settings_look_cards_classic
    CardStyle.DETAILED -> R.string.settings_look_cards_detailed
    CardStyle.MINIMAL -> R.string.settings_look_cards_minimal
}

private fun CardSize.labelRes(): Int = when (this) {
    CardSize.SMALL -> R.string.settings_look_card_size_small
    CardSize.MEDIUM -> R.string.settings_look_card_size_medium
    CardSize.LARGE -> R.string.settings_look_card_size_large
    CardSize.XL -> R.string.settings_look_card_size_xl
}

private fun Density.labelRes(): Int = when (this) {
    Density.COMFORTABLE -> R.string.settings_look_density_comfortable
    Density.COMPACT -> R.string.settings_look_density_compact
}

private fun Motion.labelRes(): Int = when (this) {
    Motion.EXPRESSIVE -> R.string.settings_look_motion_expressive
    Motion.SUBTLE -> R.string.settings_look_motion_subtle
    Motion.OFF -> R.string.settings_look_motion_off
}

private fun EpisodeView.labelRes(): Int = when (this) {
    EpisodeView.CARDS -> R.string.settings_look_episodes_cards
    EpisodeView.LIST -> R.string.settings_look_episodes_list
    EpisodeView.COMPACT -> R.string.settings_look_episodes_compact
}

private fun RelationsView.labelRes(): Int = when (this) {
    RelationsView.GRAPH -> R.string.settings_look_relations_graph
    RelationsView.LIST -> R.string.settings_look_relations_list
}

private fun CalendarView.labelRes(): Int = when (this) {
    CalendarView.WEEK -> R.string.settings_look_calendar_week
    CalendarView.AGENDA -> R.string.settings_look_calendar_agenda
    CalendarView.MONTH -> R.string.settings_look_calendar_month
    CalendarView.DAY -> R.string.settings_look_calendar_day
}

private fun RailStyle.labelRes(): Int = when (this) {
    RailStyle.LABELED -> R.string.settings_look_rail_labeled
    RailStyle.SLIM -> R.string.settings_look_rail_slim
}
