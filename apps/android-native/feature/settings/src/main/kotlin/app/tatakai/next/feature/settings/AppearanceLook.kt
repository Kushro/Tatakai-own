// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.settings

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
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
import app.tatakai.next.model.CardStyle
import app.tatakai.next.model.Density
import app.tatakai.next.model.EpisodeView
import app.tatakai.next.model.HomeLayout
import app.tatakai.next.model.LookPrefs
import app.tatakai.next.model.LookPreset
import app.tatakai.next.model.Motion
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
    LookSegmented(R.string.settings_look_cards, CardStyle.entries, look.cardStyle, { it.labelRes() }) { v -> onChange { it.copy(cardStyle = v) } }
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
