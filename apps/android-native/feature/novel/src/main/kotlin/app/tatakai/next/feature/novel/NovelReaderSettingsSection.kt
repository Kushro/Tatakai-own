// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import android.content.Context
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import app.tatakai.next.designsystem.components.Segmented
import app.tatakai.next.designsystem.components.SegmentedOption
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.Narrator
import app.tatakai.next.model.NovelReaderSettings
import app.tatakai.next.model.SecretKeys
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.TtsEngine
import app.tatakai.next.model.TtsSettings
import app.tatakai.next.model.TtsVoice
import app.tatakai.next.novel.TtsException
import app.tatakai.next.novel.TtsFailure
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import org.koin.compose.getKoin
import java.io.File
import kotlin.math.roundToInt

/**
 * Settings › Lector › Novelas (mounted right after `ReaderSettingsSection`; checklist novel.tts.settings / novel.reader.typography /
 * novel.reader.custom-font / novel.reader.flow / novel.tts.*): typography (font presets, imported TTF / OTF in `filesDir/fonts/novel/`,
 * size, line height, paragraph spacing, width, colours, justify), flow (continuous scroll, progress bar, fullscreen, keep screen on) and
 * the narrator (engine, voice with "Probar voz", speed, endpoint / model, API key in SecretStore shown as "guardada" / "Quitar").
 */
@Composable
fun NovelReaderSettingsSection(settings: AppSettings, onUpdate: (transform: (AppSettings) -> AppSettings) -> Unit, modifier: Modifier = Modifier) {
    val koin = getKoin()
    val narrator = remember { koin.getOrNull<Narrator>() }
    val secrets = remember { koin.getOrNull<SecretStore>() }
    val reader = settings.novelReader.sanitized()
    Column(modifier.fillMaxWidth()) {
        Title(stringResource(R.string.novel_settings_section))
        NovelTypographyRows(reader) { t -> onUpdate { it.copy(novelReader = t(it.novelReader).sanitized()) } }
        NovelFlowRows(reader) { t -> onUpdate { it.copy(novelReader = t(it.novelReader).sanitized()) } }
        Title(stringResource(R.string.novel_narration_section))
        NarratorRows(settings.tts.sanitized(), narrator, secrets) { t -> onUpdate { it.copy(tts = t(it.tts).sanitized()) } }
    }
}

@Composable
private fun Title(text: String) {
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

/** Font, size, line height, spacing, width, colours (also the in-reader sheet, checklist novel.reader.settings-sheet). */
@Composable
internal fun NovelTypographyRows(reader: NovelReaderSettings, onUpdate: ((NovelReaderSettings) -> NovelReaderSettings) -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var fontError by remember { mutableStateOf(false) }
    val importer = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri: Uri? ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val path = withContext(Dispatchers.IO) { runCatching { importFont(context, uri) }.getOrNull() }
            fontError = path == null
            if (path != null) onUpdate { it.copy(font = "custom", customFontPath = path) }
        }
    }
    ListItem(
        headlineContent = { Text(stringResource(R.string.novel_settings_font)) },
        supportingContent = {
            Column {
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    NOVEL_FONTS.forEach { f ->
                        if (f == "custom" && reader.customFontPath == null) return@forEach
                        FilterChip(selected = reader.font == f, onClick = { onUpdate { it.copy(font = f) } }, label = { Text(fontLabel(f)) })
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 4.dp)) {
                    TextButton(onClick = { importer.launch(arrayOf("font/ttf", "font/otf", "application/x-font-ttf", "application/x-font-otf", "font/sfnt", "application/octet-stream")) }) {
                        Text(stringResource(R.string.novel_settings_font_import))
                    }
                    val installed = reader.customFontPath
                    if (installed != null) {
                        TextButton(onClick = {
                            runCatching { File(installed).delete() }
                            onUpdate { it.copy(customFontPath = null, font = if (it.font == "custom") "serif" else it.font) }
                        }) { Text(stringResource(R.string.novel_settings_font_remove)) }
                    }
                }
                reader.customFontPath?.let { Text(stringResource(R.string.novel_settings_font_installed, File(it).name), style = MaterialTheme.typography.bodySmall) }
                if (fontError) Text(stringResource(R.string.novel_settings_font_error), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
            }
        },
    )
    SliderRow(stringResource(R.string.novel_settings_size), reader.sizeSp.toFloat(), 10f..40f, 29, label = { stringResource(R.string.novel_settings_sp, it.roundToInt()) }) { v -> onUpdate { it.copy(sizeSp = v.roundToInt()) } }
    SliderRow(stringResource(R.string.novel_settings_line_height), reader.lineHeight, 1f..2.5f, 14, label = { "%.1f×".format(it) }) { v -> onUpdate { it.copy(lineHeight = (v * 10).roundToInt() / 10f) } }
    SliderRow(stringResource(R.string.novel_settings_paragraph_spacing), reader.paragraphSpacingDp.toFloat(), 0f..48f, 23, label = { stringResource(R.string.novel_settings_dp, it.roundToInt()) }) { v ->
        onUpdate { it.copy(paragraphSpacingDp = v.roundToInt()) }
    }
    SliderRow(
        stringResource(R.string.novel_settings_width),
        reader.maxWidthDp.toFloat(),
        0f..1600f,
        31,
        label = { if (it.roundToInt() == 0) stringResource(R.string.novel_settings_no_limit) else stringResource(R.string.novel_settings_dp, it.roundToInt()) },
    ) { v -> onUpdate { it.copy(maxWidthDp = (v.roundToInt() / 50) * 50) } }
    ListItem(
        headlineContent = { Text(stringResource(R.string.novel_settings_background)) },
        supportingContent = {
            Segmented(
                options = listOf(
                    SegmentedOption("theme", stringResource(R.string.novel_bg_theme)),
                    SegmentedOption("sepia", stringResource(R.string.novel_bg_sepia)),
                    SegmentedOption("white", stringResource(R.string.novel_bg_white)),
                    SegmentedOption("gray", stringResource(R.string.novel_bg_gray)),
                    SegmentedOption("black", stringResource(R.string.novel_bg_black)),
                ),
                selected = reader.background,
                onSelect = { v -> onUpdate { it.copy(background = v) } },
                small = true,
            )
        },
    )
    SwitchRow(stringResource(R.string.novel_settings_justify), null, reader.textAlignJustify) { v -> onUpdate { it.copy(textAlignJustify = v) } }
    // Live preview with the chosen typography.
    val palette = novelPalette(reader.background)
    val family = remember(reader.font, reader.customFontPath) { novelFontFamily(reader.font, reader.customFontPath) }
    Surface(color = palette.background, shape = MaterialTheme.shapes.medium, modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
        Text(
            stringResource(R.string.novel_settings_preview_text),
            style = TextStyle(fontFamily = family, fontSize = reader.sizeSp.sp, lineHeight = (reader.sizeSp * reader.lineHeight).sp, letterSpacing = dyslexicSpacing(reader.font).em, color = palette.text),
            modifier = Modifier.padding(16.dp),
        )
    }
}

/** Continuous scroll, progress bar, fullscreen, keep the screen on (checklist novel.reader.flow). */
@Composable
internal fun NovelFlowRows(reader: NovelReaderSettings, onUpdate: ((NovelReaderSettings) -> NovelReaderSettings) -> Unit) {
    SwitchRow(stringResource(R.string.novel_settings_continuous), stringResource(R.string.novel_settings_continuous_hint), reader.continuousScroll) { v -> onUpdate { it.copy(continuousScroll = v) } }
    SwitchRow(stringResource(R.string.novel_settings_progress_bar), null, reader.showProgressBar) { v -> onUpdate { it.copy(showProgressBar = v) } }
    SwitchRow(stringResource(R.string.novel_settings_fullscreen), null, reader.fullscreenOnOpen) { v -> onUpdate { it.copy(fullscreenOnOpen = v) } }
    SwitchRow(stringResource(R.string.novel_settings_keep_on), null, reader.keepScreenOn) { v -> onUpdate { it.copy(keepScreenOn = v) } }
}

@Composable
private fun engineLabel(engine: TtsEngine): String = stringResource(
    when (engine) {
        TtsEngine.SYSTEM -> R.string.novel_narration_engine_system
        TtsEngine.OPENAI -> R.string.novel_narration_engine_openai
        TtsEngine.ELEVENLABS -> R.string.novel_narration_engine_elevenlabs
        TtsEngine.OPENAI_COMPATIBLE -> R.string.novel_narration_engine_compatible
    },
)

/** Localized cause of a narration failure (also the reader's narration bar). */
@Composable
internal fun ttsErrorText(error: Throwable): String = when ((error as? TtsException)?.reason) {
    TtsFailure.MISSING_KEY -> stringResource(R.string.novel_narration_error_key)
    TtsFailure.MISSING_VOICE -> stringResource(R.string.novel_narration_error_voice)
    TtsFailure.MISSING_MODEL -> stringResource(R.string.novel_narration_error_model)
    TtsFailure.BAD_ENDPOINT -> stringResource(R.string.novel_narration_error_endpoint)
    TtsFailure.HTTP -> stringResource(R.string.novel_narration_error_http, (error as TtsException).httpCode ?: 0)
    TtsFailure.NOT_AUDIO, TtsFailure.EMPTY_AUDIO -> stringResource(R.string.novel_narration_error_audio)
    TtsFailure.NETWORK -> stringResource(R.string.novel_narration_error_network)
    TtsFailure.ENGINE_UNAVAILABLE -> stringResource(R.string.novel_narration_error_engine)
    null -> stringResource(R.string.novel_narration_error_other, error.message ?: error::class.java.simpleName)
}

/** Narrator rows (checklist novel.tts.system / .openai / .elevenlabs / .openai-compatible / .controls). */
@Composable
private fun NarratorRows(tts: TtsSettings, narrator: Narrator?, secrets: SecretStore?, onUpdate: ((TtsSettings) -> TtsSettings) -> Unit) {
    val scope = rememberCoroutineScope()
    ListItem(
        headlineContent = { Text(stringResource(R.string.novel_narration_engine)) },
        supportingContent = {
            Segmented(
                options = TtsEngine.entries.map { SegmentedOption(it, engineLabel(it)) },
                selected = tts.engine,
                onSelect = { e -> onUpdate { it.copy(engine = e, voice = null) } },
                small = true,
            )
        },
    )
    if (tts.engine == TtsEngine.SYSTEM) {
        Text(stringResource(R.string.novel_narration_system_hint), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.padding(horizontal = 16.dp))
    }

    // Voices of the engine (system voices, OpenAI list, ElevenLabs / server voices).
    var voices by remember(tts.engine) { mutableStateOf<List<TtsVoice>?>(null) }
    var keyVersion by remember { mutableStateOf(0) }
    LaunchedEffect(tts.engine, tts.endpoint, tts.model, keyVersion, narrator) {
        voices = runCatching { narrator?.voices(tts.engine) }.getOrNull().orEmpty()
    }
    var voiceMenu by remember { mutableStateOf(false) }
    ListItem(
        headlineContent = { Text(stringResource(R.string.novel_narration_voice)) },
        supportingContent = {
            Column {
                val current = voices?.firstOrNull { it.id == tts.voice }?.label ?: tts.voice ?: stringResource(R.string.novel_narration_voice_default)
                Box {
                    OutlinedButton(onClick = { voiceMenu = true }, enabled = !voices.isNullOrEmpty()) { Text(current) }
                    DropdownMenu(expanded = voiceMenu, onDismissRequest = { voiceMenu = false }) {
                        DropdownMenuItem(text = { Text(stringResource(R.string.novel_narration_voice_default)) }, onClick = { voiceMenu = false; onUpdate { it.copy(voice = null) } })
                        voices.orEmpty().forEach { v ->
                            DropdownMenuItem(text = { Text(v.label) }, onClick = { voiceMenu = false; onUpdate { it.copy(voice = v.id) } })
                        }
                    }
                }
                if (voices != null && voices!!.isEmpty()) {
                    Text(stringResource(R.string.novel_narration_voices_empty), style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
                if (tts.engine == TtsEngine.ELEVENLABS || tts.engine == TtsEngine.OPENAI_COMPATIBLE) {
                    var manual by remember(tts.engine) { mutableStateOf(tts.voice.orEmpty()) }
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 8.dp)) {
                        OutlinedTextField(value = manual, onValueChange = { manual = it }, singleLine = true, label = { Text(stringResource(R.string.novel_narration_voice_id)) }, modifier = Modifier.weight(1f))
                        TextButton(onClick = { onUpdate { it.copy(voice = manual.trim().ifEmpty { null }) } }) { Text(stringResource(R.string.novel_save)) }
                    }
                }
            }
        },
    )
    SliderRow(stringResource(R.string.novel_narration_speed), tts.speed, 0.5f..2f, 14, label = { stringResource(R.string.novel_narration_speed_value, it) }) { v ->
        onUpdate { it.copy(speed = (v * 10).roundToInt() / 10f) }
    }
    SwitchRow(stringResource(R.string.novel_narration_highlight), null, tts.highlight) { v -> onUpdate { it.copy(highlight = v) } }

    if (tts.engine == TtsEngine.OPENAI || tts.engine == TtsEngine.OPENAI_COMPATIBLE || tts.engine == TtsEngine.ELEVENLABS) {
        var endpoint by remember(tts.endpoint) { mutableStateOf(tts.endpoint) }
        var model by remember(tts.model) { mutableStateOf(tts.model) }
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            if (tts.engine == TtsEngine.OPENAI_COMPATIBLE) {
                OutlinedTextField(
                    value = endpoint,
                    onValueChange = { endpoint = it },
                    singleLine = true,
                    label = { Text(stringResource(R.string.novel_narration_endpoint)) },
                    placeholder = { Text("http://127.0.0.1:8880/v1") },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            OutlinedTextField(
                value = model,
                onValueChange = { model = it },
                singleLine = true,
                label = { Text(stringResource(R.string.novel_narration_model)) },
                placeholder = {
                    Text(
                        when (tts.engine) {
                            TtsEngine.OPENAI -> "gpt-4o-mini-tts"
                            TtsEngine.ELEVENLABS -> "eleven_multilingual_v2"
                            else -> "kokoro"
                        },
                    )
                },
                modifier = Modifier.fillMaxWidth(),
            )
            TextButton(onClick = { onUpdate { it.copy(endpoint = endpoint.trim(), model = model.trim()) } }) { Text(stringResource(R.string.novel_save)) }
        }
        // API key (SecretStore, SecretKeys.ttsApiKey): "saved" / "remove", never shown.
        val secretKey = SecretKeys.ttsApiKey(tts.engine.id)
        var saved by remember(secretKey) { mutableStateOf(false) }
        var keyInput by remember(secretKey) { mutableStateOf("") }
        LaunchedEffect(secretKey, secrets, keyVersion) { saved = runCatching { secrets?.get(secretKey) != null }.getOrDefault(false) }
        ListItem(
            headlineContent = { Text(stringResource(R.string.novel_narration_api_key)) },
            supportingContent = {
                Column {
                    Text(
                        when {
                            secrets == null -> stringResource(R.string.novel_narration_unavailable)
                            saved -> stringResource(R.string.novel_narration_key_saved)
                            tts.engine == TtsEngine.OPENAI_COMPATIBLE -> stringResource(R.string.novel_narration_key_optional)
                            else -> stringResource(R.string.novel_narration_key_missing)
                        },
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 4.dp)) {
                        OutlinedTextField(
                            value = keyInput,
                            onValueChange = { keyInput = it },
                            singleLine = true,
                            visualTransformation = PasswordVisualTransformation(),
                            label = { Text(stringResource(R.string.novel_narration_api_key)) },
                            modifier = Modifier.weight(1f),
                        )
                        TextButton(
                            onClick = {
                                val value = keyInput.trim()
                                scope.launch {
                                    runCatching { secrets?.put(secretKey, value) }
                                    keyInput = ""
                                    keyVersion++
                                }
                            },
                            enabled = secrets != null && keyInput.isNotBlank(),
                        ) { Text(stringResource(R.string.novel_save)) }
                    }
                    if (saved) {
                        TextButton(onClick = { scope.launch { runCatching { secrets?.put(secretKey, null) }; keyVersion++ } }) { Text(stringResource(R.string.novel_narration_key_remove)) }
                    }
                }
            },
        )
    }

    // "Probar voz".
    val previewText = stringResource(R.string.novel_narration_preview_sentence)
    var testing by remember { mutableStateOf(false) }
    var testError by remember { mutableStateOf<Throwable?>(null) }
    Row(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FilledTonalButton(
            onClick = {
                testing = true
                testError = null
                scope.launch {
                    val r = try {
                        narrator?.preview(previewText) ?: Result.failure(TtsException(TtsFailure.ENGINE_UNAVAILABLE))
                    } catch (e: CancellationException) {
                        throw e
                    } catch (e: Exception) {
                        Result.failure(e)
                    }
                    testError = r.exceptionOrNull()
                    testing = false
                }
            },
            enabled = !testing,
        ) { Text(stringResource(if (testing) R.string.novel_narration_testing else R.string.novel_narration_test)) }
        if (testing) TextButton(onClick = { narrator?.stop(); testing = false }) { Text(stringResource(R.string.novel_narration_stop)) }
    }
    testError?.let { Text(ttsErrorText(it), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 16.dp)) }
}

/** Copies a picked TTF / OTF to `filesDir/fonts/novel/` (checklist novel.reader.custom-font); null when it is not a font. */
private fun importFont(context: Context, uri: Uri): String? {
    val name = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c ->
        if (c.moveToFirst()) c.getString(0) else null
    } ?: "font.ttf"
    val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return null
    if (!FontFiles.isFont(bytes)) return null
    val dir = File(context.filesDir, FontFiles.DIR).apply { mkdirs() }
    val target = File(dir, FontFiles.safeName(name, bytes))
    target.writeBytes(bytes)
    return target.absolutePath
}

/** Font file checks (JVM-tested). */
internal object FontFiles {
    const val DIR: String = "fonts/novel"

    /** TrueType (`00 01 00 00` / `true`), OpenType CFF (`OTTO`), collections (`ttcf`). */
    fun isFont(bytes: ByteArray): Boolean {
        if (bytes.size < 12) return false
        val tag = bytes.copyOfRange(0, 4)
        return tag.contentEquals(byteArrayOf(0, 1, 0, 0)) || String(tag, Charsets.ISO_8859_1) in setOf("OTTO", "true", "ttcf")
    }

    fun safeName(name: String, bytes: ByteArray): String {
        val ext = if (bytes.size >= 4 && String(bytes.copyOfRange(0, 4), Charsets.ISO_8859_1) == "OTTO") "otf" else "ttf"
        val stem = name.substringBeforeLast('.').replace(Regex("[^A-Za-z0-9_-]+"), "_").trim('_').ifEmpty { "font" }.take(48)
        return "$stem.$ext"
    }
}
