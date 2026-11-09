// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.manga.ReaderKeymap
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.ReaderDirection
import app.tatakai.next.model.ReaderFit
import app.tatakai.next.model.ReaderMode
import app.tatakai.next.model.ReaderSettings
import app.tatakai.next.model.TitleLanguage
import kotlin.math.roundToInt

/**
 * Settings › Lector (`SettingsSection.READER`; checklist manga.settings.section / manga.reader.keybinds / manga.title-language /
 * manga.reader.fit / .loading / .behavior / .modes): every `ReaderSettings` field + `AppSettings.mangaTitleLanguage` + the keymap
 * editor. The same rows (without title language and shortcuts) open inside the reader as a sheet.
 */
@Composable
fun ReaderSettingsSection(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit, modifier: Modifier = Modifier) {
    val reader = settings.reader.sanitized()
    val update: ((ReaderSettings) -> ReaderSettings) -> Unit = { t -> onUpdate { it.copy(reader = t(it.reader).sanitized()) } }
    Column(modifier.fillMaxWidth()) {
        SettingsTitle(stringResource(R.string.manga_settings_section))
        ListItem(
            headlineContent = { Text(stringResource(R.string.manga_settings_title_language)) },
            supportingContent = {
                Column {
                    Text(stringResource(R.string.manga_settings_title_language_hint))
                    Segmented(
                        options = listOf(
                            SegmentedOption(TitleLanguage.ROMAJI, stringResource(R.string.manga_title_romaji)),
                            SegmentedOption(TitleLanguage.ENGLISH, stringResource(R.string.manga_title_english)),
                            SegmentedOption(TitleLanguage.NATIVE, stringResource(R.string.manga_title_native)),
                        ),
                        selected = settings.mangaTitleLanguage,
                        onSelect = { v -> onUpdate { it.copy(mangaTitleLanguage = v) } },
                        small = true,
                    )
                }
            },
        )
        ReaderSettingsRows(reader, update)
        SettingsTitle(stringResource(R.string.manga_settings_shortcuts))
        ReaderShortcutRows(reader, update)
    }
}

@Composable
internal fun SettingsTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp))
}

@Composable
private fun SwitchRow(title: String, hint: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = if (hint != null) ({ Text(hint) }) else null,
        trailingContent = { Switch(checked = checked, onCheckedChange = onChange) },
        modifier = Modifier.clickable { onChange(!checked) },
    )
}

@Composable
private fun SliderRow(title: String, value: Float, range: ClosedFloatingPointRange<Float>, steps: Int, label: @Composable (Float) -> String, onDone: (Float) -> Unit) {
    var drag by remember(value) { mutableFloatStateOf(value) }
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = {
            Column {
                Text(label(drag))
                Slider(value = drag, onValueChange = { drag = it }, onValueChangeFinished = { onDone(drag) }, valueRange = range, steps = steps)
            }
        },
    )
}

@Composable
private fun <T> SegmentRow(title: String, options: List<SegmentedOption<T>>, selected: T, onSelect: (T) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = { Segmented(options = options, selected = selected, onSelect = onSelect, small = true) },
    )
}

/** Mode, direction, fit, layout, colours, loading and behaviour rows (Settings › Lector and the in-reader sheet). */
@Composable
internal fun ReaderSettingsRows(reader: ReaderSettings, onUpdate: ((ReaderSettings) -> ReaderSettings) -> Unit) {
    SettingsTitle(stringResource(R.string.manga_settings_layout))
    SegmentRow(
        stringResource(R.string.manga_settings_mode),
        listOf(
            SegmentedOption(ReaderMode.PAGED, stringResource(R.string.manga_mode_paged)),
            SegmentedOption(ReaderMode.VERTICAL, stringResource(R.string.manga_mode_vertical)),
            SegmentedOption(ReaderMode.WEBTOON, stringResource(R.string.manga_mode_webtoon)),
        ),
        reader.mode,
    ) { v -> onUpdate { it.copy(mode = v) } }
    SegmentRow(
        stringResource(R.string.manga_settings_direction),
        listOf(
            SegmentedOption(ReaderDirection.RTL, stringResource(R.string.manga_direction_rtl)),
            SegmentedOption(ReaderDirection.LTR, stringResource(R.string.manga_direction_ltr)),
        ),
        reader.direction,
    ) { v -> onUpdate { it.copy(direction = v) } }
    SwitchRow(stringResource(R.string.manga_settings_double_page), stringResource(R.string.manga_settings_double_page_hint), reader.doublePage) { v -> onUpdate { it.copy(doublePage = v) } }
    SegmentRow(
        stringResource(R.string.manga_settings_fit),
        listOf(
            SegmentedOption(ReaderFit.WIDTH, stringResource(R.string.manga_fit_width)),
            SegmentedOption(ReaderFit.HEIGHT, stringResource(R.string.manga_fit_height)),
            SegmentedOption(ReaderFit.BOTH, stringResource(R.string.manga_fit_both)),
            SegmentedOption(ReaderFit.NATURAL, stringResource(R.string.manga_fit_natural)),
        ),
        reader.fit,
    ) { v -> onUpdate { it.copy(fit = v) } }
    SliderRow(
        stringResource(R.string.manga_settings_max_width),
        reader.maxWidthDp.toFloat(),
        0f..2000f,
        39,
        label = { v -> if (v.roundToInt() == 0) stringResource(R.string.manga_settings_no_limit) else stringResource(R.string.manga_settings_dp, v.roundToInt()) },
    ) { v -> onUpdate { it.copy(maxWidthDp = (v.roundToInt() / 50) * 50) } }
    SliderRow(stringResource(R.string.manga_settings_gap), reader.pageGapDp.toFloat(), 0f..64f, 15, label = { v -> stringResource(R.string.manga_settings_dp, v.roundToInt()) }) { v ->
        onUpdate { it.copy(pageGapDp = v.roundToInt()) }
    }
    SwitchRow(stringResource(R.string.manga_settings_crop), stringResource(R.string.manga_settings_crop_hint), reader.cropBorders) { v -> onUpdate { it.copy(cropBorders = v) } }

    SettingsTitle(stringResource(R.string.manga_settings_colors))
    SegmentRow(
        stringResource(R.string.manga_settings_background),
        listOf(
            SegmentedOption("black", stringResource(R.string.manga_bg_black)),
            SegmentedOption("gray", stringResource(R.string.manga_bg_gray)),
            SegmentedOption("white", stringResource(R.string.manga_bg_white)),
            SegmentedOption("theme", stringResource(R.string.manga_bg_theme)),
        ),
        reader.background,
    ) { v -> onUpdate { it.copy(background = v) } }
    SwitchRow(stringResource(R.string.manga_settings_brightness_system), null, reader.brightness < 0f) { v -> onUpdate { it.copy(brightness = if (v) -1f else 0.6f) } }
    if (reader.brightness >= 0f) {
        SliderRow(stringResource(R.string.manga_settings_brightness), reader.brightness, 0.05f..1f, 18, label = { v -> "${(v * 100).roundToInt()}%" }) { v ->
            onUpdate { it.copy(brightness = v) }
        }
    }

    SettingsTitle(stringResource(R.string.manga_settings_loading))
    SwitchRow(stringResource(R.string.manga_settings_eager), stringResource(R.string.manga_settings_eager_hint), reader.eagerLoading) { v -> onUpdate { it.copy(eagerLoading = v) } }
    if (!reader.eagerLoading) {
        SliderRow(stringResource(R.string.manga_settings_prefetch), reader.prefetchPages.toFloat(), 1f..20f, 18, label = { v -> stringResource(R.string.manga_settings_pages, v.roundToInt()) }) { v ->
            onUpdate { it.copy(prefetchPages = v.roundToInt()) }
        }
    }
    SwitchRow(stringResource(R.string.manga_settings_infinite), stringResource(R.string.manga_settings_infinite_hint), reader.infiniteScroll) { v -> onUpdate { it.copy(infiniteScroll = v) } }

    SettingsTitle(stringResource(R.string.manga_settings_behavior))
    SliderRow(
        stringResource(R.string.manga_settings_auto_scroll),
        reader.autoScrollSpeed.toFloat(),
        0f..600f,
        29,
        label = { v -> if (v.roundToInt() == 0) stringResource(R.string.manga_settings_off) else stringResource(R.string.manga_settings_px_s, v.roundToInt()) },
    ) { v -> onUpdate { it.copy(autoScrollSpeed = (v.roundToInt() / 20) * 20) } }
    SwitchRow(stringResource(R.string.manga_settings_fullscreen), null, reader.fullscreenOnOpen) { v -> onUpdate { it.copy(fullscreenOnOpen = v) } }
    SwitchRow(stringResource(R.string.manga_settings_hide_chrome), stringResource(R.string.manga_settings_hide_chrome_hint), reader.hideChromeOnTap) { v -> onUpdate { it.copy(hideChromeOnTap = v) } }
    SwitchRow(stringResource(R.string.manga_settings_progress), null, reader.showProgress) { v -> onUpdate { it.copy(showProgress = v) } }
    SwitchRow(stringResource(R.string.manga_settings_zoom_keys), stringResource(R.string.manga_settings_zoom_keys_hint), reader.zoomShortcuts) { v -> onUpdate { it.copy(zoomShortcuts = v) } }
}

@Composable
private fun actionLabel(action: String): String = stringResource(
    when (action) {
        ReaderKeymap.NEXT_PAGE -> R.string.manga_key_next_page
        ReaderKeymap.PREV_PAGE -> R.string.manga_key_prev_page
        ReaderKeymap.SCROLL_DOWN -> R.string.manga_key_scroll_down
        ReaderKeymap.SCROLL_UP -> R.string.manga_key_scroll_up
        ReaderKeymap.NEXT_CHAPTER -> R.string.manga_key_next_chapter
        ReaderKeymap.PREV_CHAPTER -> R.string.manga_key_prev_chapter
        ReaderKeymap.TOGGLE_FULLSCREEN -> R.string.manga_key_fullscreen
        ReaderKeymap.TOGGLE_MODE -> R.string.manga_key_mode
        ReaderKeymap.ZOOM_IN -> R.string.manga_key_zoom_in
        ReaderKeymap.ZOOM_OUT -> R.string.manga_key_zoom_out
        ReaderKeymap.TOGGLE_CHROME -> R.string.manga_key_chrome
        else -> R.string.manga_key_reload
    },
)

@Composable
private fun keyLabel(key: String): String = when (key) {
    "SPACE" -> stringResource(R.string.manga_key_space)
    "DPAD_RIGHT" -> "→"
    "DPAD_LEFT" -> "←"
    "DPAD_UP" -> "↑"
    "DPAD_DOWN" -> "↓"
    "EQUALS" -> "="
    "MINUS" -> "−"
    else -> key.replace('_', ' ')
}

/** Shortcut editor (checklist manga.reader.keybinds): tap an action, press a key; conflicts are marked; "Restablecer". */
@Composable
private fun ReaderShortcutRows(reader: ReaderSettings, onUpdate: ((ReaderSettings) -> ReaderSettings) -> Unit) {
    val resolved = ReaderKeymap.resolve(reader.keymap)
    val conflicts = ReaderKeymap.conflicts(reader.keymap)
    var editing by remember { mutableStateOf<String?>(null) }
    Text(stringResource(R.string.manga_settings_shortcuts_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp))
    ReaderKeymap.ACTIONS.forEach { action ->
        val key = resolved.getValue(action)
        ListItem(
            headlineContent = { Text(actionLabel(action)) },
            supportingContent = if (key in conflicts) ({ Text(stringResource(R.string.manga_settings_shortcut_conflict), color = MaterialTheme.colorScheme.error) }) else null,
            trailingContent = {
                Surface(shape = RoundedCornerShape(8.dp), color = MaterialTheme.colorScheme.secondaryContainer) {
                    Text(keyLabel(key), Modifier.padding(horizontal = 10.dp, vertical = 4.dp), style = MaterialTheme.typography.labelLarge)
                }
            },
            modifier = Modifier.clickable { editing = action },
        )
    }
    TextButton(onClick = { onUpdate { it.copy(keymap = ReaderKeymap.reset()) } }, enabled = reader.keymap.isNotEmpty(), modifier = Modifier.padding(horizontal = 8.dp)) {
        Text(stringResource(R.string.manga_settings_shortcuts_reset))
    }
    HorizontalDivider(Modifier.padding(top = 8.dp))
    editing?.let { action ->
        var captured by remember(action) { mutableStateOf<String?>(null) }
        val focus = remember { FocusRequester() }
        LaunchedEffect(action) { runCatching { focus.requestFocus() } }
        val k = captured
        val reserved = k != null && k in ReaderKeymap.RESERVED
        val clashes = if (k != null && !reserved) ReaderKeymap.conflictsWith(reader.keymap, action, k) else emptyList()
        AlertDialog(
            onDismissRequest = { editing = null },
            title = { Text(actionLabel(action)) },
            text = {
                Column(
                    Modifier.focusRequester(focus).focusable().onKeyEvent { e ->
                        if (e.type == KeyEventType.KeyDown) {
                            captured = ReaderKeymap.keyName(android.view.KeyEvent.keyCodeToString(e.nativeKeyEvent.keyCode))
                            true
                        } else {
                            false
                        }
                    },
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Text(stringResource(R.string.manga_settings_shortcut_press))
                    Text(k?.let { keyLabel(it) } ?: stringResource(R.string.manga_settings_shortcut_waiting), style = MaterialTheme.typography.headlineSmall)
                    if (reserved) Text(stringResource(R.string.manga_settings_shortcut_reserved), color = MaterialTheme.colorScheme.error)
                    if (clashes.isNotEmpty()) {
                        Text(stringResource(R.string.manga_settings_shortcut_clash, clashes.map { actionLabelPlain(it) }.joinToString(", ")), color = MaterialTheme.colorScheme.error)
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    val key = k ?: return@TextButton
                    onUpdate { it.copy(keymap = ReaderKeymap.bind(it.keymap, action, key)) }
                    editing = null
                }, enabled = k != null && !reserved) { Text(stringResource(R.string.manga_settings_save)) }
            },
            dismissButton = { TextButton(onClick = { editing = null }) { Text(stringResource(app.tatakai.next.ui.R.string.ui_cancel)) } },
        )
    }
}

@Composable
private fun actionLabelPlain(action: String): String = actionLabel(action)
