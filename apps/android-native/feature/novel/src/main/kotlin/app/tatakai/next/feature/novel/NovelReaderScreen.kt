// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.outlined.List
import androidx.compose.material.icons.outlined.Fullscreen
import androidx.compose.material.icons.outlined.FullscreenExit
import androidx.compose.material.icons.outlined.Pause
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.RecordVoiceOver
import androidx.compose.material.icons.outlined.SkipNext
import androidx.compose.material.icons.outlined.SkipPrevious
import androidx.compose.material.icons.outlined.Stop
import androidx.compose.material.icons.outlined.TextFields
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Slider
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.manga.ChapterText
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.NovelReaderArgs
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.model.TtsState
import app.tatakai.next.novel.NovelChapters
import app.tatakai.next.ui.state.userMessage
import kotlinx.coroutines.flow.distinctUntilChanged
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf
import kotlin.math.roundToInt
import app.tatakai.next.ui.R as UiR

/**
 * `Route.NovelReader(args)` (checklist novel.reader.* / novel.tts.*): texto con la tipografía de `NovelReaderSettings`, scroll continuo
 * entre capítulos, barra de progreso + porcentaje + número de capítulo, pantalla completa, mantener pantalla encendida, hoja de ajustes
 * sin salir, "Leer en voz alta" con `Narrator` (pausa / reanudar / detener / velocidad, párrafo resaltado), guardar posición al salir.
 * Offline with `args.offline` (`offlineText`).
 */
@Composable
fun NovelReaderScreen(
    args: NovelReaderArgs,
    onBack: () -> Unit,
    onReplace: (NovelReaderArgs) -> Unit,
    onOpenSettings: (SettingsSection) -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: NovelReaderViewModel = koinViewModel(key = "novel-reader-${args.extensionId}-${args.sourceId}-${args.chapterId}", parameters = { parametersOf(args) })
    val ui by vm.ui.collectAsStateWithLifecycle()
    val reader by vm.reader.collectAsStateWithLifecycle()
    val settings by vm.settings.collectAsStateWithLifecycle()
    val ttsState by vm.ttsState.collectAsStateWithLifecycle()
    val ttsParagraph by vm.ttsParagraph.collectAsStateWithLifecycle()
    val ttsError by vm.ttsError.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val view = LocalView.current
    val activity = remember(context) { context.findActivity() }
    val snackbar = remember { SnackbarHostState() }
    var chrome by remember { mutableStateOf(true) }
    var fullscreen by remember { mutableStateOf(reader.fullscreenOnOpen) }
    var sheet by remember { mutableStateOf<NovelSheet?>(null) }
    val lastChapter = stringResource(R.string.novel_reader_last_chapter)

    LaunchedEffect(vm) {
        vm.events.collect { ev ->
            when (ev) {
                is NovelReaderEvent.Replace -> onReplace(ev.args)
                NovelReaderEvent.LastChapter -> snackbar.showSnackbar(lastChapter)
            }
        }
    }
    BackHandler {
        vm.saveProgress()
        onBack()
    }
    DisposableEffect(activity) {
        onDispose {
            view.keepScreenOn = false
            activity?.window?.let { WindowCompat.getInsetsController(it, view).show(WindowInsetsCompat.Type.systemBars()) }
        }
    }
    LaunchedEffect(reader.keepScreenOn, ttsState) { view.keepScreenOn = reader.keepScreenOn || ttsState == TtsState.SPEAKING }
    LaunchedEffect(fullscreen, chrome, activity) {
        val window = activity?.window ?: return@LaunchedEffect
        val insets = WindowCompat.getInsetsController(window, view)
        if (fullscreen && !chrome) {
            insets.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
            insets.hide(WindowInsetsCompat.Type.systemBars())
        } else {
            insets.show(WindowInsetsCompat.Type.systemBars())
        }
    }

    val palette = novelPalette(reader.background)
    val family = remember(reader.font, reader.customFontPath) { novelFontFamily(reader.font, reader.customFontPath) }
    val textStyle = TextStyle(
        fontFamily = family,
        fontSize = reader.sizeSp.sp,
        lineHeight = (reader.sizeSp * reader.lineHeight).sp,
        letterSpacing = dyslexicSpacing(reader.font).em,
        color = palette.text,
        textAlign = if (reader.textAlignJustify) TextAlign.Justify else TextAlign.Start,
    )

    // Flattened rows: (segment, paragraph); paragraph -1 = chapter title, -2 = loading / error, end row = (-1, -3).
    val rows = remember(ui.segments) {
        buildList {
            ui.segments.forEachIndexed { s, seg ->
                add(s to -1)
                when (val t = seg.text) {
                    is LoadState.Ok -> t.value.paragraphs.indices.forEach { add(s to it) }
                    else -> add(s to -2)
                }
            }
            add(-1 to -3)
        }
    }
    val list = rememberLazyListState(initialFirstVisibleItemIndex = (args.startParagraph + 1).coerceAtLeast(0))
    LaunchedEffect(list, rows) {
        snapshotFlow { list.firstVisibleItemIndex }.distinctUntilChanged().collect { i ->
            val (s, p) = rows.getOrNull(i) ?: return@collect
            if (s >= 0) vm.onPosition(s, p.coerceAtLeast(0))
        }
    }
    // Continuous scroll: the next chapter is appended when the end row shows up.
    LaunchedEffect(list, rows.size, reader.continuousScroll) {
        snapshotFlow { (list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0) >= rows.size - 2 }.distinctUntilChanged().collect { atEnd ->
            if (atEnd && reader.continuousScroll && ui.segments.lastOrNull()?.text is LoadState.Ok) vm.appendNext()
        }
    }
    // Narration follows the paragraph being read (scrolls when highlight is on).
    LaunchedEffect(ttsParagraph, ui.narrating) {
        val p = ttsParagraph ?: return@LaunchedEffect
        val s = ui.narrating ?: return@LaunchedEffect
        val index = rows.indexOfFirst { it.first == s && it.second == p }
        if (index >= 0 && settings.tts.highlight) list.animateScrollToItem(index)
    }

    Box(modifier.fillMaxSize().background(palette.background)) {
        val first = ui.segments.firstOrNull()
        if (first != null && ui.segments.size == 1 && first.text is LoadState.Loading) {
            CircularProgressIndicator(Modifier.align(Alignment.Center))
        } else if (first != null && ui.segments.size == 1 && first.text is LoadState.Err) {
            Column(Modifier.align(Alignment.Center).padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Text(stringResource(R.string.novel_reader_failed), color = palette.text, style = MaterialTheme.typography.titleMedium)
                Text((first.text as LoadState.Err).cause.userMessage(), color = palette.text, textAlign = TextAlign.Center)
                Button(onClick = vm::retry, modifier = Modifier.padding(top = 12.dp)) { Text(stringResource(UiR.string.ui_retry)) }
            }
        } else {
            LazyColumn(
                state = list,
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(reader.paragraphSpacingDp.dp),
                contentPadding = PaddingValues(top = 96.dp, bottom = 160.dp, start = 20.dp, end = 20.dp),
                modifier = Modifier.fillMaxSize().pointerInput(Unit) { detectTapGestures(onTap = { chrome = !chrome }) },
            ) {
                itemsIndexed(rows, key = { i, r -> "${r.first}:${r.second}:$i" }) { _, (s, p) ->
                    val width = if (reader.maxWidthDp > 0) Modifier.widthIn(max = reader.maxWidthDp.dp) else Modifier
                    when {
                        s == -1 -> Column(width.fillMaxWidth().padding(vertical = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                            Text(stringResource(R.string.novel_reader_end), color = palette.text, style = MaterialTheme.typography.titleMedium)
                            if (vm.hasNext && !reader.continuousScroll) {
                                Button(onClick = vm::goNext, modifier = Modifier.padding(top = 12.dp)) { Text(stringResource(R.string.novel_reader_next_chapter)) }
                            } else if (!vm.hasNext) {
                                Text(stringResource(R.string.novel_reader_last_chapter), color = palette.text, style = MaterialTheme.typography.bodyMedium)
                            }
                        }
                        p == -1 -> {
                            val seg = ui.segments[s]
                            val title = (seg.text as? LoadState.Ok)?.value?.title ?: seg.chapter.title
                            Column(width.fillMaxWidth().padding(top = if (s == 0) 0.dp else 48.dp, bottom = 8.dp)) {
                                ChapterText.formatNumber(seg.chapter.number)?.let {
                                    Text(stringResource(R.string.novel_chapter_n, it), color = palette.text.copy(alpha = 0.7f), style = MaterialTheme.typography.labelLarge)
                                }
                                title?.let { Text(it, color = palette.text, style = textStyle.copy(fontSize = (reader.sizeSp + 6).sp, lineHeight = ((reader.sizeSp + 6) * 1.3f).sp)) }
                            }
                        }
                        p == -2 -> when (val t = ui.segments[s].text) {
                            is LoadState.Err -> Column(width.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                                Text(t.cause.userMessage(), color = palette.text)
                                OutlinedButton(onClick = vm::retry) { Text(stringResource(UiR.string.ui_retry)) }
                            }
                            else -> Box(width.fillMaxWidth().height(160.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                        }
                        else -> {
                            val text = (ui.segments[s].text as LoadState.Ok).value.paragraphs[p]
                            val highlighted = settings.tts.highlight && ui.narrating == s && ttsParagraph == p && ttsState != TtsState.IDLE
                            Text(
                                text,
                                style = textStyle,
                                modifier = width.fillMaxWidth()
                                    .then(if (highlighted) Modifier.background(palette.highlight, MaterialTheme.shapes.small) else Modifier)
                                    .clickable(interactionSource = remember { MutableInteractionSource() }, indication = null) {
                                        // A tap on a paragraph while narrating restarts the narration there.
                                        if (ttsState != TtsState.IDLE) vm.speak(s, p) else chrome = !chrome
                                    },
                            )
                        }
                    }
                }
            }
        }

        if (reader.showProgressBar) {
            LinearProgressIndicator(progress = { ui.fraction }, modifier = Modifier.fillMaxWidth().align(Alignment.BottomCenter).navigationBarsPadding())
        }

        AnimatedVisibility(visible = chrome, enter = fadeIn(), exit = fadeOut(), modifier = Modifier.align(Alignment.TopCenter)) {
            Surface(color = MaterialTheme.colorScheme.surfaceContainer.copy(alpha = 0.95f), modifier = Modifier.fillMaxWidth()) {
                Row(Modifier.statusBarsPadding().padding(4.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = { vm.saveProgress(); onBack() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) }
                    Column(Modifier.weight(1f)) {
                        Text(args.title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        Text(
                            listOfNotNull(
                                ChapterText.formatNumber(ui.chapter?.number)?.let { stringResource(R.string.novel_chapter_n, it) },
                                stringResource(R.string.novel_reader_percent, (ui.fraction * 100).roundToInt()),
                            ).joinToString(" · "),
                            style = MaterialTheme.typography.labelMedium,
                        )
                    }
                    IconButton(onClick = { sheet = NovelSheet.CHAPTERS }) { Icon(Icons.AutoMirrored.Outlined.List, contentDescription = stringResource(R.string.novel_reader_chapters)) }
                    IconButton(onClick = { sheet = NovelSheet.SETTINGS }) { Icon(Icons.Outlined.TextFields, contentDescription = stringResource(R.string.novel_reader_settings)) }
                    IconButton(onClick = { fullscreen = !fullscreen }) {
                        Icon(if (fullscreen) Icons.Outlined.FullscreenExit else Icons.Outlined.Fullscreen, contentDescription = stringResource(R.string.novel_reader_fullscreen))
                    }
                }
            }
        }
        AnimatedVisibility(visible = chrome || ttsState != TtsState.IDLE, enter = fadeIn(), exit = fadeOut(), modifier = Modifier.align(Alignment.BottomCenter)) {
            Surface(color = MaterialTheme.colorScheme.surfaceContainer.copy(alpha = 0.95f), modifier = Modifier.fillMaxWidth()) {
                Column(Modifier.navigationBarsPadding().padding(horizontal = 8.dp, vertical = 4.dp)) {
                    ttsError?.let { err ->
                        Text(ttsErrorText(err), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 8.dp))
                    }
                    if (ttsState != TtsState.IDLE) {
                        var speed by remember(settings.tts.speed) { mutableFloatStateOf(settings.tts.speed) }
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(stringResource(R.string.novel_narration_speed_value, speed), style = MaterialTheme.typography.labelMedium, modifier = Modifier.width(56.dp))
                            Slider(value = speed, onValueChange = { speed = it }, onValueChangeFinished = { vm.setSpeed((speed * 10).roundToInt() / 10f) }, valueRange = 0.5f..2f, steps = 14, modifier = Modifier.weight(1f))
                        }
                    }
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        IconButton(onClick = vm::goPrevious, enabled = vm.hasPrevious) { Icon(Icons.Outlined.SkipPrevious, contentDescription = stringResource(R.string.novel_reader_previous_chapter)) }
                        when (ttsState) {
                            TtsState.IDLE -> TextButton(onClick = { vm.speak() }) {
                                Icon(Icons.Outlined.RecordVoiceOver, contentDescription = null)
                                Text(stringResource(R.string.novel_narration_read_aloud), modifier = Modifier.padding(start = 6.dp))
                            }
                            TtsState.PREPARING -> CircularProgressIndicator(Modifier.padding(horizontal = 12.dp).width(24.dp).height(24.dp), strokeWidth = 2.dp)
                            TtsState.SPEAKING -> IconButton(onClick = vm::pause) { Icon(Icons.Outlined.Pause, contentDescription = stringResource(R.string.novel_narration_pause)) }
                            TtsState.PAUSED -> IconButton(onClick = vm::resume) { Icon(Icons.Outlined.PlayArrow, contentDescription = stringResource(R.string.novel_narration_resume)) }
                        }
                        if (ttsState != TtsState.IDLE) {
                            IconButton(onClick = vm::stopNarration) { Icon(Icons.Outlined.Stop, contentDescription = stringResource(R.string.novel_narration_stop)) }
                        }
                        Text(
                            if (ui.paragraphs.isNotEmpty()) stringResource(R.string.novel_reader_percent, (ui.fraction * 100).roundToInt()) else "",
                            style = MaterialTheme.typography.labelLarge,
                            textAlign = TextAlign.Center,
                            modifier = Modifier.weight(1f),
                        )
                        IconButton(onClick = vm::goNext, enabled = vm.hasNext) { Icon(Icons.Outlined.SkipNext, contentDescription = stringResource(R.string.novel_reader_next_chapter)) }
                    }
                }
            }
        }
        SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).padding(bottom = 120.dp))
    }

    when (sheet) {
        NovelSheet.SETTINGS -> AdaptiveSheet(onDismissRequest = { sheet = null }) {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(stringResource(R.string.novel_reader_settings), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(16.dp))
                NovelTypographyRows(reader, onUpdate = vm::updateReader)
                NovelFlowRows(reader, onUpdate = vm::updateReader)
                TextButton(onClick = { sheet = null; onOpenSettings(SettingsSection.READER) }, modifier = Modifier.padding(8.dp)) {
                    Text(stringResource(R.string.novel_reader_all_settings))
                }
            }
        }
        NovelSheet.CHAPTERS -> AdaptiveSheet(onDismissRequest = { sheet = null }) {
            Text(stringResource(R.string.novel_reader_chapters), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(16.dp))
            val chapters = NovelChapters.sorted(ui.chapters, ascending = true)
            if (chapters.isEmpty()) {
                Text(stringResource(R.string.novel_chapters_empty), modifier = Modifier.padding(16.dp))
            } else {
                val state = rememberLazyListState(initialFirstVisibleItemIndex = chapters.indexOfFirst { it.id == ui.chapter?.id }.coerceAtLeast(0))
                LazyColumn(state = state, modifier = Modifier.heightIn(max = 480.dp)) {
                    itemsIndexed(chapters, key = { _, c -> c.id }) { _, c ->
                        ListItem(
                            headlineContent = {
                                Text(
                                    (ChapterText.formatNumber(c.number)?.let { stringResource(R.string.novel_chapter_n, it) } ?: c.id) + (c.title?.let { " · $it" } ?: ""),
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    color = if (c.id == ui.chapter?.id) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                                )
                            },
                            supportingContent = { Text(listOfNotNull(c.translator, c.lang?.uppercase()).joinToString(" · ")) },
                            modifier = Modifier.clickable { sheet = null; vm.open(c) },
                        )
                    }
                }
            }
        }
        null -> Unit
    }
}

private enum class NovelSheet { SETTINGS, CHAPTERS }

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}
