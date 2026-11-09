// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Delete
import androidx.compose.material.icons.rounded.FontDownload
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.SecretKeys
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.SubtitleTranslation
import app.tatakai.next.model.TranslationMode
import app.tatakai.next.model.TranslationProvider
import app.tatakai.next.player.PlayerKeymap
import app.tatakai.next.player.PlayerPrefs
import app.tatakai.next.player.SubtitleExtras
import app.tatakai.next.player.SubtitleTools
import app.tatakai.next.player.SubtitleTranslators
import app.tatakai.next.player.TranslationException
import app.tatakai.next.player.TranslatorConfig
import app.tatakai.next.ui.format.formatBytes
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.koin.compose.getKoin
import kotlin.math.roundToInt

/**
 * Parity wave A contract (docs/redesign/15-parity-wave-a-contract-android.md §3.2). Owner: worker p. Mounted by Settings › Reproductor
 * (worker q's module) right after the "Saltar intro" switch, so p owns its own settings rows AND their strings (`player_settings_*` in
 * :feature:player) without touching :feature:settings.
 *
 * Rows: auto-next countdown (3..30 s), stable volume, background playback, secondary subtitle language, advanced subtitle style (colour,
 * outline, position, imported fonts, preview), subtitle translation (provider, mode, target language, endpoint / model / region, API key
 * in SecretStore, "Probar", "Vaciar caché") and the keyboard shortcuts editor (`AppSettings.playerKeymap`, conflicts, "Restablecer").
 * Settings changes go through [onUpdate] (saves immediately and shows "Ajustes guardados"); the style extras live in [PlayerPrefs].
 */
@Composable
fun PlayerSettingsExtras(
    settings: AppSettings,
    onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit,
    modifier: Modifier = Modifier,
) {
    val koin = getKoin()
    val prefs = remember { koin.getOrNull<PlayerPrefs>() }
    val tools = remember { koin.getOrNull<SubtitleTools>() }
    val secrets = remember { koin.getOrNull<SecretStore>() }
    Column(modifier.fillMaxWidth()) {
        PlaybackRows(settings, onUpdate)
        SectionTitle(stringResource(R.string.player_settings_subs_section))
        SecondaryLanguageRow(settings, onUpdate)
        if (prefs != null) SubtitleStyleExtrasRows(settings, prefs, tools)
        SectionTitle(stringResource(R.string.player_settings_translation_section))
        TranslationRows(settings, onUpdate, tools, secrets)
        SectionTitle(stringResource(R.string.player_settings_shortcuts_section))
        ShortcutRows(settings, onUpdate)
    }
}

@Composable
private fun SectionTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(start = 16.dp, top = 16.dp, bottom = 4.dp))
}

@Composable
private fun Hint(text: String, color: Color = MaterialTheme.colorScheme.onSurfaceVariant) {
    Text(text, style = MaterialTheme.typography.bodySmall, color = color, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp))
}

@Composable
private fun SwitchItem(title: String, hint: String?, checked: Boolean, onChange: (Boolean) -> Unit) {
    ListItem(
        headlineContent = { Text(title) },
        supportingContent = if (hint != null) ({ Text(hint) }) else null,
        trailingContent = { Switch(checked = checked, onCheckedChange = onChange) },
        modifier = Modifier.clickable { onChange(!checked) },
    )
}

// ---- playback ---------------------------------------------------------------------------------------------------------------------------

@Composable
private fun PlaybackRows(settings: AppSettings, onUpdate: ((AppSettings) -> AppSettings) -> Unit) {
    val secs = settings.autoNextCountdownSecs.coerceIn(3, 30)
    var drag by remember(secs) { mutableFloatStateOf(secs.toFloat()) }
    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_countdown)) },
        supportingContent = {
            Column {
                Text(
                    if (settings.autoNext) stringResource(R.string.player_settings_countdown_value, drag.roundToInt()) else stringResource(R.string.player_settings_countdown_off),
                )
                Slider(
                    value = drag,
                    onValueChange = { drag = it },
                    onValueChangeFinished = { val v = drag.roundToInt().coerceIn(3, 30); onUpdate { it.copy(autoNextCountdownSecs = v) } },
                    valueRange = 3f..30f,
                    steps = 26,
                    enabled = settings.autoNext,
                )
            }
        },
    )
    SwitchItem(stringResource(R.string.player_stable_volume), stringResource(R.string.player_stable_volume_hint), settings.stableVolume) { v ->
        onUpdate { it.copy(stableVolume = v) }
    }
    SwitchItem(stringResource(R.string.player_settings_background), stringResource(R.string.player_settings_background_hint), settings.backgroundPlayback) { v ->
        onUpdate { it.copy(backgroundPlayback = v) }
    }
}

// ---- subtitles --------------------------------------------------------------------------------------------------------------------------

private val SECONDARY_LANGS = listOf("en", "es", "ja", "pt", "fr", "de", "it", "ko", "zh", "ru", "ar")

@Composable
private fun SecondaryLanguageRow(settings: AppSettings, onUpdate: ((AppSettings) -> AppSettings) -> Unit) {
    ListItem(
        headlineContent = { Text(stringResource(R.string.player_sub_secondary)) },
        supportingContent = {
            Column {
                Text(stringResource(R.string.player_settings_secondary_hint))
                Row(Modifier.horizontalScroll(rememberScrollState()).padding(top = 4.dp), Arrangement.spacedBy(6.dp)) {
                    FilterChip(selected = settings.secondarySubLang == null, onClick = { onUpdate { it.copy(secondarySubLang = null) } }, label = { Text(stringResource(R.string.player_sub_secondary_off)) })
                    SECONDARY_LANGS.forEach { l ->
                        FilterChip(selected = settings.secondarySubLang == l, onClick = { onUpdate { it.copy(secondarySubLang = l) } }, label = { Text(l.uppercase()) })
                    }
                }
            }
        },
    )
}

@Composable
private fun ColorSwatches(colors: List<Int>, selected: Int, onPick: (Int) -> Unit) {
    val scheme = MaterialTheme.colorScheme
    Row(Modifier.horizontalScroll(rememberScrollState()).padding(top = 4.dp), Arrangement.spacedBy(10.dp)) {
        colors.forEach { c ->
            Box(
                Modifier
                    .size(32.dp)
                    .background(Color(c), CircleShape)
                    .border(BorderStroke(if (c == selected) 3.dp else 1.dp, if (c == selected) scheme.primary else scheme.outline), CircleShape)
                    .clickable { onPick(c) },
                contentAlignment = Alignment.Center,
            ) {
                if (c == selected) Icon(Icons.Rounded.Check, contentDescription = null, tint = if (c == SubtitleExtras.BLACK) Color(SubtitleExtras.WHITE) else Color(SubtitleExtras.BLACK), modifier = Modifier.size(18.dp))
            }
        }
    }
}

/** Colour, outline, vertical position, imported fonts and a live preview (player.subs.style.*, player.subs.custom-fonts). */
@Composable
private fun SubtitleStyleExtrasRows(settings: AppSettings, prefs: PlayerPrefs, tools: SubtitleTools?) {
    val extras by prefs.subtitleExtras.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val context = LocalContext.current
    var fonts by remember { mutableStateOf(tools?.fonts?.list().orEmpty()) }
    var fontError by remember { mutableStateOf(false) }
    val importer = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) { uris: List<Uri> ->
        val t = tools ?: return@rememberLauncherForActivityResult
        scope.launch {
            var failed = false
            withContext(Dispatchers.IO) {
                uris.forEach { uri ->
                    val name = t.displayName(uri) ?: "font.ttf"
                    val stored = runCatching { context.contentResolver.openInputStream(uri)?.let { t.fonts.import(name, it) } }.getOrNull()
                    if (stored == null) failed = true
                }
            }
            fontError = failed
            fonts = t.fonts.list()
        }
    }

    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_text_color)) },
        supportingContent = { ColorSwatches(SubtitleExtras.TEXT_COLORS, extras.textColor) { c -> prefs.updateSubtitleExtras { it.copy(textColor = c) } } },
    )
    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_outline_color)) },
        supportingContent = { ColorSwatches(SubtitleExtras.OUTLINE_COLORS, extras.outlineColor) { c -> prefs.updateSubtitleExtras { it.copy(outlineColor = c) } } },
    )
    var outline by remember(extras.outlineSize) { mutableFloatStateOf(extras.outlineSize) }
    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_outline_size, outline.roundToInt())) },
        supportingContent = {
            Column {
                if (!settings.subtitleStyle.outline) Text(stringResource(R.string.player_settings_outline_disabled))
                Slider(
                    value = outline,
                    onValueChange = { outline = it },
                    onValueChangeFinished = { val v = outline; prefs.updateSubtitleExtras { it.copy(outlineSize = v) } },
                    valueRange = 0f..SubtitleExtras.MAX_OUTLINE,
                    steps = 5,
                )
            }
        },
    )
    var position by remember(extras.positionPct) { mutableFloatStateOf(extras.positionPct.toFloat()) }
    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_position, position.roundToInt())) },
        supportingContent = {
            Column {
                Text(stringResource(R.string.player_settings_position_hint))
                Slider(
                    value = position,
                    onValueChange = { position = it },
                    onValueChangeFinished = { val v = position.roundToInt(); prefs.updateSubtitleExtras { it.copy(positionPct = v) } },
                    valueRange = 0f..100f,
                )
            }
        },
    )

    // Imported fonts: choose one for subtitles (mpv `sub-font`, Media3 drawing), delete, import TTF / OTF.
    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_fonts)) },
        supportingContent = { Text(stringResource(R.string.player_settings_fonts_hint)) },
        leadingContent = { Icon(Icons.Rounded.FontDownload, contentDescription = null) },
    )
    if (tools == null) {
        Hint(stringResource(R.string.player_settings_unavailable))
    } else {
        FontChoice(stringResource(R.string.player_settings_font_default), extras.fontFile == null) { prefs.updateSubtitleExtras { it.copy(fontFile = null) } }
        if (fonts.isEmpty()) Hint(stringResource(R.string.player_settings_fonts_empty))
        fonts.forEach { f ->
            FontChoice(
                f.name,
                extras.fontFile == f.name,
                onDelete = {
                    tools.fonts.delete(f.name)
                    if (extras.fontFile == f.name) prefs.updateSubtitleExtras { it.copy(fontFile = null) }
                    fonts = tools.fonts.list()
                },
            ) { prefs.updateSubtitleExtras { it.copy(fontFile = f.name) } }
        }
        if (fontError) Hint(stringResource(R.string.player_settings_fonts_error), MaterialTheme.colorScheme.error)
        TextButton(onClick = { importer.launch(arrayOf("font/ttf", "font/otf", "application/x-font-ttf", "application/x-font-otf", "application/octet-stream")) }, modifier = Modifier.padding(horizontal = 8.dp)) {
            Text(stringResource(R.string.player_settings_fonts_import))
        }
    }

    // Live preview with the full style (player.subs.style.preview).
    Text(stringResource(R.string.player_settings_preview), style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(start = 16.dp, top = 8.dp))
    Surface(
        Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp),
        shape = RoundedCornerShape(16.dp),
        color = MaterialTheme.colorScheme.inverseSurface,
    ) {
        Box(Modifier.padding(vertical = 24.dp, horizontal = 12.dp), contentAlignment = Alignment.Center) {
            SubtitleLines(listOf(stringResource(R.string.player_settings_preview_line)), settings.subtitleStyle, extras, tools?.fonts?.file(extras.fontFile))
        }
    }
}

@Composable
private fun FontChoice(name: String, selected: Boolean, onDelete: (() -> Unit)? = null, onSelect: () -> Unit) {
    ListItem(
        headlineContent = { Text(name) },
        leadingContent = { if (selected) Icon(Icons.Rounded.Check, contentDescription = null, tint = MaterialTheme.colorScheme.primary) },
        trailingContent = if (onDelete != null) ({ IconButton(onClick = onDelete) { Icon(Icons.Rounded.Delete, stringResource(R.string.player_settings_fonts_delete)) } }) else null,
        modifier = Modifier.clickable(onClick = onSelect),
    )
}

// ---- translation ------------------------------------------------------------------------------------------------------------------------

private fun TranslationProvider.needsKey(): Boolean = this == TranslationProvider.DEEPL || this == TranslationProvider.AZURE
private fun TranslationProvider.acceptsKey(): Boolean = this != TranslationProvider.NONE && this != TranslationProvider.GOOGLE
private fun TranslationProvider.usesEndpoint(): Boolean = this == TranslationProvider.DEEPLX || this == TranslationProvider.OPENAI || this == TranslationProvider.AZURE

/** Translation rows (player.subs.translate.settings): the key goes to SecretStore under `SecretKeys.translationApiKey`, never shown. */
@Composable
private fun TranslationRows(settings: AppSettings, onUpdate: ((AppSettings) -> AppSettings) -> Unit, tools: SubtitleTools?, secrets: SecretStore?) {
    val t = settings.subtitleTranslation
    val scope = rememberCoroutineScope()
    fun update(transform: (SubtitleTranslation) -> SubtitleTranslation) = onUpdate { it.copy(subtitleTranslation = transform(it.subtitleTranslation)) }

    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_provider)) },
        supportingContent = {
            Row(Modifier.horizontalScroll(rememberScrollState()).padding(top = 4.dp), Arrangement.spacedBy(6.dp)) {
                TranslationProvider.entries.forEach { p ->
                    FilterChip(selected = t.provider == p, onClick = { update { it.copy(provider = p) } }, label = { Text(providerLabel(p)) })
                }
            }
        },
    )
    if (t.provider == TranslationProvider.NONE) {
        Hint(stringResource(R.string.player_settings_translation_off))
        return
    }
    if (t.provider == TranslationProvider.GOOGLE) Hint(stringResource(R.string.player_settings_google_hint))
    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_mode)) },
        supportingContent = {
            Column {
                Row(Modifier.padding(top = 4.dp), Arrangement.spacedBy(6.dp)) {
                    FilterChip(selected = t.mode == TranslationMode.LIVE, onClick = { update { it.copy(mode = TranslationMode.LIVE) } }, label = { Text(stringResource(R.string.player_settings_mode_live)) })
                    FilterChip(selected = t.mode == TranslationMode.PRELOAD, onClick = { update { it.copy(mode = TranslationMode.PRELOAD) } }, label = { Text(stringResource(R.string.player_settings_mode_preload)) })
                }
                Text(stringResource(if (t.mode == TranslationMode.LIVE) R.string.player_settings_mode_live_hint else R.string.player_settings_mode_preload_hint))
            }
        },
    )
    ListItem(
        headlineContent = { Text(stringResource(R.string.player_settings_target)) },
        supportingContent = {
            Row(Modifier.horizontalScroll(rememberScrollState()).padding(top = 4.dp), Arrangement.spacedBy(6.dp)) {
                SubtitleTranslators.TARGET_LANGUAGES.forEach { l ->
                    FilterChip(selected = t.targetLang == l, onClick = { update { it.copy(targetLang = l) } }, label = { Text(l.uppercase()) })
                }
            }
        },
    )

    // Connection fields, saved together.
    if (t.provider.usesEndpoint() || t.provider == TranslationProvider.OPENAI) {
        var endpoint by remember(t.provider, t.endpoint) { mutableStateOf(t.endpoint) }
        var model by remember(t.provider, t.model) { mutableStateOf(t.model) }
        var region by remember(t.provider, t.region) { mutableStateOf(t.region) }
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (t.provider.usesEndpoint()) {
                OutlinedTextField(
                    value = endpoint,
                    onValueChange = { endpoint = it },
                    label = { Text(stringResource(if (t.provider == TranslationProvider.AZURE) R.string.player_settings_endpoint_optional else R.string.player_settings_endpoint)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            if (t.provider == TranslationProvider.OPENAI) {
                OutlinedTextField(value = model, onValueChange = { model = it }, label = { Text(stringResource(R.string.player_settings_model)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
            }
            if (t.provider == TranslationProvider.AZURE) {
                OutlinedTextField(value = region, onValueChange = { region = it }, label = { Text(stringResource(R.string.player_settings_region)) }, singleLine = true, modifier = Modifier.fillMaxWidth())
            }
            val dirty = endpoint != t.endpoint || model != t.model || region != t.region
            FilledTonalButton(onClick = { update { it.copy(endpoint = endpoint.trim(), model = model.trim(), region = region.trim()) } }, enabled = dirty) {
                Text(stringResource(R.string.player_settings_save))
            }
        }
    }

    // API key (SecretStore): "saved" / "remove", the field empties after saving.
    if (t.provider.acceptsKey()) {
        var keySaved by remember(t.provider) { mutableStateOf<Boolean?>(null) }
        var keyInput by remember(t.provider) { mutableStateOf("") }
        var keyError by remember(t.provider) { mutableStateOf(false) }
        val secretKey = SecretKeys.translationApiKey(t.provider.id)
        LaunchedEffect(t.provider, secrets) { keySaved = runCatching { secrets?.get(secretKey) != null }.getOrDefault(false) }
        ListItem(
            headlineContent = { Text(stringResource(if (t.provider.needsKey()) R.string.player_settings_key else R.string.player_settings_key_optional)) },
            supportingContent = {
                Text(
                    when {
                        secrets == null -> stringResource(R.string.player_settings_unavailable)
                        keySaved == true -> stringResource(R.string.player_settings_key_saved)
                        else -> stringResource(R.string.player_settings_key_none)
                    },
                )
            },
            trailingContent = if (keySaved == true && secrets != null) {
                {
                    TextButton(onClick = { scope.launch { runCatching { secrets.put(secretKey, null) }; keySaved = false } }) {
                        Text(stringResource(R.string.player_settings_key_remove))
                    }
                }
            } else {
                null
            },
        )
        if (secrets != null) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedTextField(
                    value = keyInput,
                    onValueChange = { keyInput = it; keyError = false },
                    label = { Text(stringResource(R.string.player_settings_key_input)) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    isError = keyError,
                    modifier = Modifier.weight(1f),
                )
                FilledTonalButton(
                    onClick = {
                        val v = keyInput.trim()
                        scope.launch {
                            val ok = runCatching { secrets.put(secretKey, v) }.isSuccess
                            keyError = !ok
                            if (ok) {
                                keyInput = ""
                                keySaved = true
                            }
                        }
                    },
                    enabled = keyInput.isNotBlank(),
                ) { Text(stringResource(R.string.player_settings_save)) }
            }
        }
    }

    // "Probar": one sample line through the configured provider.
    var testing by remember(t) { mutableStateOf(false) }
    var testResult by remember(t) { mutableStateOf<String?>(null) }
    var testFailed by remember(t) { mutableStateOf(false) }
    val missingKeyText = stringResource(R.string.player_settings_test_missing)
    Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        FilledTonalButton(
            enabled = tools != null && !testing,
            onClick = {
                val tl = tools ?: return@FilledTonalButton
                testing = true
                testResult = null
                scope.launch {
                    val key = runCatching { secrets?.get(SecretKeys.translationApiKey(t.provider.id)) }.getOrNull()
                    val result = try {
                        Result.success(tl.translator(TranslatorConfig.from(t, key)).translate(listOf(SubtitleTranslators.TEST_LINE)).firstOrNull().orEmpty())
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: TranslationException) {
                        Result.failure(e)
                    } catch (e: Exception) {
                        Result.failure(e)
                    }
                    testing = false
                    testFailed = result.isFailure
                    testResult = result.fold(
                        onSuccess = { it },
                        onFailure = { e ->
                            if (e is TranslationException && e.kind != TranslationException.Kind.HTTP && e.kind != TranslationException.Kind.FORMAT) missingKeyText else (e.message ?: e.javaClass.simpleName)
                        },
                    )
                }
            },
        ) { Text(stringResource(if (testing) R.string.player_settings_testing else R.string.player_settings_test)) }
        testResult?.let {
            Text(
                if (testFailed) stringResource(R.string.player_settings_test_failed, it) else stringResource(R.string.player_settings_test_ok, it),
                style = MaterialTheme.typography.bodySmall,
                color = if (testFailed) MaterialTheme.colorScheme.error else MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.weight(1f),
            )
        }
    }

    // Cache (player.subs.translate.cache).
    if (tools != null) {
        var size by remember { mutableLongStateOf(tools.translationCache.sizeBytes()) }
        ListItem(
            headlineContent = { Text(stringResource(R.string.player_settings_cache)) },
            supportingContent = { Text(stringResource(R.string.player_settings_cache_size, formatBytes(size))) },
            trailingContent = {
                TextButton(onClick = { scope.launch { tools.translationCache.clear(); size = tools.translationCache.sizeBytes() } }, enabled = size > 0) {
                    Text(stringResource(R.string.player_settings_cache_clear))
                }
            },
        )
    }
}

// ---- keyboard shortcuts -----------------------------------------------------------------------------------------------------------------

/** Editor of `AppSettings.playerKeymap` (player.shortcuts.rebind / .reset): conflicts are shown before saving. */
@Composable
private fun ShortcutRows(settings: AppSettings, onUpdate: ((AppSettings) -> AppSettings) -> Unit) {
    val resolved = PlayerKeymap.resolve(settings.playerKeymap)
    val conflicts = PlayerKeymap.conflicts(settings.playerKeymap)
    var editing by remember { mutableStateOf<String?>(null) }
    Hint(stringResource(R.string.player_settings_shortcuts_hint))
    PlayerKeymap.ACTIONS.forEach { action ->
        val key = resolved.getValue(action)
        ListItem(
            headlineContent = { Text(actionLabel(action)) },
            supportingContent = if (key in conflicts) ({ Text(stringResource(R.string.player_settings_shortcut_conflict), color = MaterialTheme.colorScheme.error) }) else null,
            trailingContent = {
                Surface(shape = RoundedCornerShape(8.dp), color = MaterialTheme.colorScheme.secondaryContainer, contentColor = MaterialTheme.colorScheme.onSecondaryContainer) {
                    Text(keyLabel(key), Modifier.padding(horizontal = 10.dp, vertical = 4.dp), style = MaterialTheme.typography.labelLarge)
                }
            },
            modifier = Modifier.clickable { editing = action },
        )
    }
    TextButton(onClick = { onUpdate { it.copy(playerKeymap = PlayerKeymap.reset()) } }, enabled = settings.playerKeymap.isNotEmpty(), modifier = Modifier.padding(horizontal = 8.dp)) {
        Text(stringResource(R.string.player_settings_shortcuts_reset))
    }
    HorizontalDivider(Modifier.padding(top = 8.dp))

    editing?.let { action ->
        KeyCaptureDialog(
            action = action,
            keymap = settings.playerKeymap,
            onSave = { key -> onUpdate { it.copy(playerKeymap = PlayerKeymap.bind(it.playerKeymap, action, key)) }; editing = null },
            onDismiss = { editing = null },
        )
    }
}

@Composable
private fun KeyCaptureDialog(action: String, keymap: Map<String, String>, onSave: (String) -> Unit, onDismiss: () -> Unit) {
    var captured by remember { mutableStateOf<String?>(null) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    val key = captured
    val reserved = key != null && key in PlayerKeymap.RESERVED
    val clashes = if (key != null && !reserved) PlayerKeymap.conflictsWith(keymap, action, key) else emptyList()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(actionLabel(action)) },
        text = {
            Column(
                Modifier.focusRequester(focus).focusable().onKeyEvent { e ->
                    if (e.type == KeyEventType.KeyDown) {
                        captured = keyNameOf(e.nativeKeyEvent.keyCode)
                        true
                    } else {
                        false
                    }
                },
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Text(stringResource(R.string.player_settings_shortcut_press))
                Text(
                    key?.let { keyLabel(it) } ?: stringResource(R.string.player_settings_shortcut_waiting),
                    style = MaterialTheme.typography.headlineSmall,
                )
                if (reserved) Text(stringResource(R.string.player_settings_shortcut_reserved), color = MaterialTheme.colorScheme.error)
                if (clashes.isNotEmpty()) {
                    Text(stringResource(R.string.player_settings_shortcut_clash, clashes.map { actionLabelText(it) }.joinToString(", ")), color = MaterialTheme.colorScheme.error)
                }
            }
        },
        confirmButton = { TextButton(onClick = { key?.let(onSave) }, enabled = key != null && !reserved) { Text(stringResource(R.string.player_settings_save)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.player_cancel)) } },
    )
}

@Composable
private fun actionLabelText(action: String): String = actionLabel(action)
