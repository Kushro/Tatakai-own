// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import android.app.Activity
import android.content.Context
import android.content.ContextWrapper
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.gestures.animateScrollBy
import androidx.compose.foundation.gestures.calculatePan
import androidx.compose.foundation.gestures.calculateZoom
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.scrollBy
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.outlined.CameraAlt
import androidx.compose.material.icons.outlined.Fullscreen
import androidx.compose.material.icons.outlined.FullscreenExit
import androidx.compose.material.icons.automirrored.outlined.List
import androidx.compose.material.icons.outlined.MoreVert
import androidx.compose.material.icons.outlined.Pause
import androidx.compose.material.icons.outlined.PlayArrow
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.SkipNext
import androidx.compose.material.icons.outlined.SkipPrevious
import androidx.compose.material.icons.outlined.SwapHoriz
import androidx.compose.material.icons.outlined.ViewCarousel
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
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
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.input.pointer.PointerEventType
import androidx.compose.ui.input.pointer.isCtrlPressed
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import app.tatakai.next.designsystem.components.AdaptiveSheet
import app.tatakai.next.manga.ChapterLists
import app.tatakai.next.manga.ChapterOrder
import app.tatakai.next.manga.ChapterText
import app.tatakai.next.manga.ReaderKeymap
import app.tatakai.next.manga.key
import app.tatakai.next.model.ChapterPage
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MangaReaderArgs
import app.tatakai.next.model.ReaderDirection
import app.tatakai.next.model.ReaderFit
import app.tatakai.next.model.ReaderMode
import app.tatakai.next.model.ReaderSettings
import app.tatakai.next.model.SettingsSection
import app.tatakai.next.ui.state.userMessage
import coil3.compose.AsyncImage
import coil3.compose.AsyncImagePainter
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch
import org.koin.compose.viewmodel.koinViewModel
import org.koin.core.parameter.parametersOf
import app.tatakai.next.ui.R as UiR

/**
 * `Route.MangaReader(args)` (checklist manga.reader.*): paginado / vertical / webtoon, doble página, LTR/RTL, ajuste, fondo, brillo,
 * recorte, carga ansiosa / perezosa + precarga (70 % → siguiente capítulo), scroll infinito, carril de miniaturas, navegador de
 * capítulos, auto-scroll, zoom (pellizco, doble toque, Ctrl+rueda, teclas), pantalla completa, ocultar chrome, progreso, captura,
 * cambio de extensión si fallan las páginas, guardar progreso al salir, atajos reasignables. Offline with `args.offline`.
 */
@Composable
fun MangaReaderScreen(
    args: MangaReaderArgs,
    onBack: () -> Unit,
    onReplace: (MangaReaderArgs) -> Unit,
    onOpenSettings: (SettingsSection) -> Unit,
    modifier: Modifier = Modifier,
) {
    val vm: MangaReaderViewModel = koinViewModel(key = "reader-${args.extensionId}-${args.sourceId}-${args.chapterId}", parameters = { parametersOf(args) })
    val ui by vm.ui.collectAsStateWithLifecycle()
    val reader by vm.reader.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val view = LocalView.current
    val activity = remember(context) { context.findActivity() }
    val scope = rememberCoroutineScope()
    val snackbar = remember { SnackbarHostState() }

    var chrome by remember { mutableStateOf(true) }
    var fullscreen by remember { mutableStateOf(reader.fullscreenOnOpen) }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    var autoScroll by remember { mutableStateOf(false) }
    var sheet by remember { mutableStateOf<ReaderSheet?>(null) }
    var jumpTo by remember { mutableStateOf<Int?>(null) }
    var stepRequest by remember { mutableIntStateOf(0) }
    val focus = remember { FocusRequester() }

    val noMatch = stringResource(R.string.manga_reader_no_match)
    val noNext = stringResource(R.string.manga_reader_last_chapter)
    val savedTo = stringResource(R.string.manga_reader_captured)
    val captureFailed = stringResource(R.string.manga_reader_capture_failed)
    val failedText = stringResource(UiR.string.ui_error_title)
    LaunchedEffect(vm) {
        vm.events.collect { ev ->
            when (ev) {
                is ReaderEvent.Replace -> onReplace(ev.args)
                ReaderEvent.NoMatch -> snackbar.showSnackbar(noMatch)
                ReaderEvent.NoNextChapter -> snackbar.showSnackbar(noNext)
                is ReaderEvent.Failed -> snackbar.showSnackbar(failedText)
            }
        }
    }

    BackHandler {
        vm.saveProgress()
        onBack()
    }

    // Window: immersive while fullscreen, brightness override, screen kept on while reading.
    DisposableEffect(activity) {
        val window = activity?.window
        val previousBrightness = window?.attributes?.screenBrightness
        view.keepScreenOn = true
        onDispose {
            view.keepScreenOn = false
            window?.let { WindowCompat.getInsetsController(it, view).show(WindowInsetsCompat.Type.systemBars()) }
            if (window != null && previousBrightness != null) window.attributes = window.attributes.also { it.screenBrightness = previousBrightness }
        }
    }
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
    LaunchedEffect(reader.brightness, activity) {
        val window = activity?.window ?: return@LaunchedEffect
        window.attributes = window.attributes.also { it.screenBrightness = if (reader.brightness >= 0f) reader.brightness.coerceIn(0.02f, 1f) else -1f }
    }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }

    // Cache warming: eager = whole chapter, lazy = the next N pages; and the next chapter once prefetched at 70 %.
    val pages = ui.pages
    LaunchedEffect(pages, ui.page, reader.eagerLoading, reader.prefetchPages, reader.cropBorders) {
        if (pages.isEmpty()) return@LaunchedEffect
        val range = ReaderMath.warmRange(ui.page, pages.size, reader.eagerLoading, reader.prefetchPages)
        warmPages(context, pages.slice(range), reader.cropBorders)
    }
    LaunchedEffect(ui.prefetched) {
        if (ui.prefetched.isNotEmpty()) warmPages(context, ui.prefetched.take(reader.prefetchPages.coerceAtLeast(3)), reader.cropBorders)
    }

    val background = when (reader.background) {
        "white" -> Color.White
        "gray" -> Color(0xFF424242)
        "theme" -> MaterialTheme.colorScheme.background
        else -> Color.Black
    }
    val onBackground = if (reader.background == "white") Color.Black else if (reader.background == "theme") MaterialTheme.colorScheme.onBackground else Color.White

    fun toggleChrome() {
        if (reader.hideChromeOnTap) chrome = !chrome
    }
    fun zoomBy(factor: Float) {
        scale = (scale * factor).coerceIn(1f, 5f)
        if (scale == 1f) offset = Offset.Zero
    }

    fun handleAction(action: String): Boolean {
        when (action) {
            ReaderKeymap.NEXT_PAGE, ReaderKeymap.SCROLL_DOWN -> stepRequest = 1
            ReaderKeymap.PREV_PAGE, ReaderKeymap.SCROLL_UP -> stepRequest = -1
            ReaderKeymap.NEXT_CHAPTER -> vm.goNext()
            ReaderKeymap.PREV_CHAPTER -> vm.goPrevious()
            ReaderKeymap.TOGGLE_FULLSCREEN -> fullscreen = !fullscreen
            ReaderKeymap.TOGGLE_MODE -> vm.updateReader { it.copy(mode = ReaderMode.entries[(it.mode.ordinal + 1) % ReaderMode.entries.size]) }
            ReaderKeymap.ZOOM_IN -> if (reader.zoomShortcuts) zoomBy(1.25f) else return false
            ReaderKeymap.ZOOM_OUT -> if (reader.zoomShortcuts) zoomBy(0.8f) else return false
            ReaderKeymap.TOGGLE_CHROME -> chrome = !chrome
            ReaderKeymap.RELOAD -> vm.retry()
            else -> return false
        }
        return true
    }

    Box(
        modifier
            .fillMaxSize()
            .background(background)
            .focusRequester(focus)
            .focusable()
            .onPreviewKeyEvent { e ->
                if (e.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                val key = ReaderKeymap.keyName(android.view.KeyEvent.keyCodeToString(e.nativeKeyEvent.keyCode))
                if (key in ReaderKeymap.RESERVED) return@onPreviewKeyEvent false
                val action = ReaderKeymap.actionFor(reader.keymap, key) ?: return@onPreviewKeyEvent false
                handleAction(action)
            },
    ) {
        val segment = ui.segments.getOrNull(0)
        when {
            segment == null -> Unit
            segment.pages is LoadState.Loading && ui.segments.size == 1 -> CircularProgressIndicator(Modifier.align(Alignment.Center))
            segment.pages is LoadState.Err && ui.segments.size == 1 -> PagesError(
                message = (segment.pages as LoadState.Err).cause.userMessage(),
                onRetry = vm::retry,
                onSwitch = { sheet = ReaderSheet.SOURCES; if (ui.alternatives == null) vm.loadAlternatives() },
                color = onBackground,
                modifier = Modifier.align(Alignment.Center),
            )
            else -> ZoomBox(
                scale = scale,
                offset = offset,
                ctrlWheel = reader.zoomShortcuts,
                onTransform = { s, o -> scale = s; offset = o },
                onTap = { fraction ->
                    // Paged: left / right thirds turn the page (mirrored in RTL), the middle toggles the chrome.
                    if (reader.mode == ReaderMode.PAGED && scale == 1f) {
                        val rtl = reader.direction == ReaderDirection.RTL
                        when {
                            fraction < 0.33f -> stepRequest = if (rtl) 1 else -1
                            fraction > 0.67f -> stepRequest = if (rtl) -1 else 1
                            else -> toggleChrome()
                        }
                    } else {
                        toggleChrome()
                    }
                },
            ) {
                if (reader.mode == ReaderMode.PAGED) {
                    PagedReader(
                        ui = ui,
                        reader = reader,
                        startPage = args.startPage,
                        scrollEnabled = scale == 1f,
                        step = stepRequest,
                        jumpTo = jumpTo,
                        onStepConsumed = { stepRequest = 0 },
                        onJumpConsumed = { jumpTo = null },
                        onPage = { vm.onPage(0, it) },
                        onEnd = vm::goNext,
                        hasNext = vm.hasNext,
                        onError = vm::onPageError,
                        onLoaded = vm::onPageLoaded,
                        onSwitch = { sheet = ReaderSheet.SOURCES; if (ui.alternatives == null) vm.loadAlternatives() },
                        textColor = onBackground,
                    )
                } else {
                    ScrollReader(
                        ui = ui,
                        reader = reader,
                        startPage = args.startPage,
                        autoScroll = autoScroll && reader.autoScrollSpeed > 0,
                        step = stepRequest,
                        jumpTo = jumpTo,
                        onStepConsumed = { stepRequest = 0 },
                        onJumpConsumed = { jumpTo = null },
                        onPage = vm::onPage,
                        onAppendNext = vm::appendNext,
                        onNext = vm::goNext,
                        hasNext = vm.hasNext,
                        onError = vm::onPageError,
                        onLoaded = vm::onPageLoaded,
                        onSwitch = { sheet = ReaderSheet.SOURCES; if (ui.alternatives == null) vm.loadAlternatives() },
                        textColor = onBackground,
                    )
                }
            }
        }

        // Progress indicator (always, small) when the chrome is hidden.
        if (reader.showProgress && !chrome && pages.isNotEmpty()) {
            Text(
                "${ui.page + 1} / ${pages.size}",
                color = onBackground,
                style = MaterialTheme.typography.labelMedium,
                modifier = Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(8.dp)
                    .background(background.copy(alpha = 0.6f), MaterialTheme.shapes.small).padding(horizontal = 8.dp, vertical = 2.dp),
            )
        }

        AnimatedVisibility(visible = chrome, enter = fadeIn(), exit = fadeOut(), modifier = Modifier.align(Alignment.TopCenter)) {
            TopChrome(
                title = args.title,
                chapterLine = ui.chapter?.let { c -> listOfNotNull(ChapterText.formatNumber(c.number)?.let { stringResource(R.string.manga_chapter_n, it) }, c.title).joinToString(" · ") }.orEmpty(),
                sourceName = ui.source?.extensionName,
                fullscreen = fullscreen,
                onBack = {
                    vm.saveProgress()
                    onBack()
                },
                onFullscreen = { fullscreen = !fullscreen },
                onSettings = { sheet = ReaderSheet.SETTINGS },
                onChapters = { sheet = ReaderSheet.CHAPTERS },
                onSources = { sheet = ReaderSheet.SOURCES; vm.loadAlternatives() },
                onCapture = {
                    val page = pages.getOrNull(ui.page) ?: return@TopChrome
                    scope.launch {
                        val path = runCatching { capturePage(context, page, "${args.title}-${ChapterText.formatNumber(ui.chapter?.number) ?: "x"}-${ui.page + 1}") }.getOrNull()
                        snackbar.showSnackbar(if (path != null) savedTo.format(path) else captureFailed)
                    }
                },
                onOpenSettings = { onOpenSettings(SettingsSection.READER) },
            )
        }
        AnimatedVisibility(visible = chrome, enter = fadeIn(), exit = fadeOut(), modifier = Modifier.align(Alignment.BottomCenter)) {
            BottomChrome(
                ui = ui,
                reader = reader,
                hasPrevious = vm.hasPrevious,
                hasNext = vm.hasNext,
                autoScroll = autoScroll,
                onAutoScroll = { autoScroll = !autoScroll },
                onPrevious = vm::goPrevious,
                onNext = vm::goNext,
                onJump = { jumpTo = it },
                onRail = { sheet = ReaderSheet.RAIL },
            )
        }
        SnackbarHost(snackbar, Modifier.align(Alignment.BottomCenter).padding(bottom = 96.dp))
    }

    when (sheet) {
        ReaderSheet.SETTINGS -> AdaptiveSheet(onDismissRequest = { sheet = null }) {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(stringResource(R.string.manga_reader_settings), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(16.dp))
                ReaderSettingsRows(reader, onUpdate = vm::updateReader)
                TextButton(onClick = { sheet = null; onOpenSettings(SettingsSection.READER) }, modifier = Modifier.padding(8.dp)) {
                    Text(stringResource(R.string.manga_reader_all_settings))
                }
            }
        }
        ReaderSheet.CHAPTERS -> AdaptiveSheet(onDismissRequest = { sheet = null }) {
            Text(stringResource(R.string.manga_reader_chapters), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(16.dp))
            val list = ChapterLists.sorted(ui.chapters, ChapterOrder.ASC)
            if (list.isEmpty()) {
                Text(stringResource(R.string.manga_chapters_empty), modifier = Modifier.padding(16.dp))
            } else {
                val listState = rememberLazyListState(initialFirstVisibleItemIndex = list.indexOfFirst { it.id == ui.chapter?.id }.coerceAtLeast(0))
                LazyColumn(state = listState, modifier = Modifier.heightIn(max = 480.dp)) {
                    itemsIndexed(list, key = { _, c -> c.id }) { _, c ->
                        ListItem(
                            headlineContent = {
                                Text(
                                    (ChapterText.formatNumber(c.number)?.let { stringResource(R.string.manga_chapter_n, it) } ?: c.id) + (c.title?.let { " · $it" } ?: ""),
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                    color = if (c.id == ui.chapter?.id) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface,
                                )
                            },
                            supportingContent = { Text(listOfNotNull(c.group, c.lang?.uppercase()).joinToString(" · ")) },
                            modifier = Modifier.clickable { sheet = null; vm.open(c) },
                        )
                    }
                }
            }
        }
        ReaderSheet.SOURCES -> AdaptiveSheet(onDismissRequest = { sheet = null }) {
            Text(stringResource(R.string.manga_reader_switch_source), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(16.dp))
            Text(stringResource(R.string.manga_reader_switch_hint), style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 16.dp))
            when (val alts = ui.alternatives) {
                null, LoadState.Loading -> Box(Modifier.fillMaxWidth().padding(24.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                is LoadState.Err -> ErrorBlock(alts.cause, vm::loadAlternatives)
                is LoadState.Ok -> if (alts.value.isEmpty()) {
                    Text(stringResource(R.string.manga_no_sources), modifier = Modifier.padding(16.dp))
                } else {
                    alts.value.forEach { s ->
                        ListItem(
                            headlineContent = { Text(s.extensionName, color = if (s.key == ui.source?.key) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurface) },
                            supportingContent = { Text(listOfNotNull(s.chapterCount?.toString(), s.languages.joinToString("/").uppercase().ifBlank { null }, "${(s.reliability * 100).toInt()}%").joinToString(" · ")) },
                            modifier = Modifier.clickable(enabled = s.key != ui.source?.key) { sheet = null; vm.switchSource(s) },
                        )
                    }
                }
            }
        }
        ReaderSheet.RAIL -> AdaptiveSheet(onDismissRequest = { sheet = null }) {
            Text(stringResource(R.string.manga_reader_pages), style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(16.dp))
            val railState = rememberLazyListState(initialFirstVisibleItemIndex = (ui.page - 2).coerceAtLeast(0))
            LazyRow(state = railState, contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                itemsIndexed(pages, key = { i, p -> "$i-${p.url}" }) { i, p ->
                    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.width(72.dp).clickable { sheet = null; jumpTo = i }) {
                        AsyncImage(
                            model = pageRequest(context, p, cropBorders = false),
                            contentDescription = null,
                            contentScale = ContentScale.Crop,
                            modifier = Modifier.width(72.dp).height(104.dp).background(MaterialTheme.colorScheme.surfaceContainerHighest)
                                .then(if (i == ui.page) Modifier.border(2.dp, MaterialTheme.colorScheme.primary) else Modifier),
                        )
                        Text("${i + 1}", style = MaterialTheme.typography.labelSmall)
                    }
                }
            }
        }
        null -> Unit
    }
}

private enum class ReaderSheet { SETTINGS, CHAPTERS, SOURCES, RAIL }

private tailrec fun Context.findActivity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.findActivity()
    else -> null
}

/**
 * Pinch zoom + pan (only while zoomed, so swipes still turn pages), double tap toggles 1x / 2.5x (checklist manga.reader.zoom),
 * Ctrl + wheel zooms on keyboards (manga.reader.behavior). [onTap] gets the horizontal tap position 0..1.
 */
@Composable
private fun ZoomBox(
    scale: Float,
    offset: Offset,
    ctrlWheel: Boolean,
    onTransform: (Float, Offset) -> Unit,
    onTap: (Float) -> Unit,
    content: @Composable () -> Unit,
) {
    BoxWithConstraints(Modifier.fillMaxSize()) {
        val widthPx = constraints.maxWidth.toFloat().coerceAtLeast(1f)
        val heightPx = constraints.maxHeight.toFloat().coerceAtLeast(1f)
        fun clamp(s: Float, o: Offset): Offset {
            val maxX = (widthPx * (s - 1f)) / 2f
            val maxY = (heightPx * (s - 1f)) / 2f
            return Offset(o.x.coerceIn(-maxX, maxX), o.y.coerceIn(-maxY, maxY))
        }
        val currentScale by androidx.compose.runtime.rememberUpdatedState(scale)
        val currentOffset by androidx.compose.runtime.rememberUpdatedState(offset)
        Box(
            Modifier
                .fillMaxSize()
                .pointerInput(Unit) {
                    detectTapGestures(
                        onDoubleTap = { pos ->
                            if (currentScale > 1f) {
                                onTransform(1f, Offset.Zero)
                            } else {
                                val s = 2.5f
                                val center = Offset(widthPx / 2f, heightPx / 2f)
                                onTransform(s, clamp(s, (center - pos) * (s - 1f)))
                            }
                        },
                        onTap = { pos -> onTap(pos.x / widthPx) },
                    )
                }
                .pointerInput(Unit) {
                    awaitPointerEventScope {
                        while (true) {
                            val event = awaitPointerEvent()
                            if (event.type == PointerEventType.Scroll) {
                                if (ctrlWheel && event.keyboardModifiers.isCtrlPressed) {
                                    val dy = event.changes.firstOrNull()?.scrollDelta?.y ?: 0f
                                    val s = (currentScale * if (dy < 0) 1.1f else 0.9f).coerceIn(1f, 5f)
                                    onTransform(s, if (s == 1f) Offset.Zero else clamp(s, currentOffset))
                                    event.changes.forEach { it.consume() }
                                }
                                continue
                            }
                            val pointers = event.changes.count { it.pressed }
                            if (pointers >= 2 || (currentScale > 1f && pointers == 1)) {
                                val zoom = event.calculateZoom()
                                val pan = event.calculatePan()
                                val s = (currentScale * zoom).coerceIn(1f, 5f)
                                val o = if (s == 1f) Offset.Zero else clamp(s, currentOffset + pan)
                                if (s != currentScale || o != currentOffset) onTransform(s, o)
                                event.changes.forEach { if (it.positionChanged()) it.consume() }
                            }
                        }
                    }
                }
                .graphicsLayer {
                    scaleX = scale
                    scaleY = scale
                    translationX = offset.x
                    translationY = offset.y
                },
        ) {
            content()
        }
    }
}

private fun androidx.compose.ui.input.pointer.PointerInputChange.positionChanged(): Boolean = position != previousPosition

@Composable
private fun PagedReader(
    ui: MangaReaderUi,
    reader: ReaderSettings,
    startPage: Int,
    scrollEnabled: Boolean,
    step: Int,
    jumpTo: Int?,
    onStepConsumed: () -> Unit,
    onJumpConsumed: () -> Unit,
    onPage: (Int) -> Unit,
    onEnd: () -> Unit,
    hasNext: Boolean,
    onError: (String) -> Unit,
    onLoaded: (String) -> Unit,
    onSwitch: () -> Unit,
    textColor: Color,
) {
    val pages = ui.pages
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // Double page only makes sense on wide screens (landscape / tablets).
        val double = reader.doublePage && maxWidth > maxHeight
        val spreads = remember(pages.size, double) { ReaderMath.spreads(pages.size, double) }
        val initial = remember(spreads) { ReaderMath.spreadOf(spreads, startPage.coerceIn(0, (pages.size - 1).coerceAtLeast(0))) }
        val pager = rememberPagerState(initialPage = initial, pageCount = { spreads.size + 1 })
        LaunchedEffect(pager, spreads) {
            snapshotFlow { pager.currentPage }.distinctUntilChanged().collect { index ->
                val spread = spreads.getOrNull(index)
                if (spread != null) onPage(spread.last()) else if (pages.isNotEmpty()) onPage(pages.size - 1)
            }
        }
        LaunchedEffect(step) {
            if (step == 0) return@LaunchedEffect
            val target = (pager.currentPage + step).coerceIn(0, spreads.size)
            if (target == pager.currentPage && step > 0 && pager.currentPage == spreads.size) onEnd()
            pager.animateScrollToPage(target)
            onStepConsumed()
        }
        LaunchedEffect(jumpTo) {
            val j = jumpTo ?: return@LaunchedEffect
            pager.scrollToPage(ReaderMath.spreadOf(spreads, j))
            onJumpConsumed()
        }
        HorizontalPager(
            state = pager,
            reverseLayout = reader.direction == ReaderDirection.RTL,
            userScrollEnabled = scrollEnabled,
            beyondViewportPageCount = if (reader.eagerLoading) 3 else 1,
            modifier = Modifier.fillMaxSize(),
        ) { index ->
            val spread = spreads.getOrNull(index)
            if (spread == null) {
                EndCard(hasNext = hasNext, onNext = onEnd, color = textColor)
            } else {
                Row(
                    Modifier.fillMaxSize(),
                    horizontalArrangement = Arrangement.Center,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    val ordered = if (reader.direction == ReaderDirection.RTL) spread.reversed() else spread
                    ordered.forEach { i ->
                        Box(Modifier.weight(1f).fillMaxHeight(), contentAlignment = Alignment.Center) {
                            PageImage(pages[i], i, reader, inList = false, failed = pages[i].url in ui.failedPages, onError = onError, onLoaded = onLoaded, onSwitch = onSwitch, textColor = textColor)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ScrollReader(
    ui: MangaReaderUi,
    reader: ReaderSettings,
    startPage: Int,
    autoScroll: Boolean,
    step: Int,
    jumpTo: Int?,
    onStepConsumed: () -> Unit,
    onJumpConsumed: () -> Unit,
    onPage: (Int, Int) -> Unit,
    onAppendNext: () -> Unit,
    onNext: () -> Unit,
    hasNext: Boolean,
    onError: (String) -> Unit,
    onLoaded: (String) -> Unit,
    onSwitch: () -> Unit,
    textColor: Color,
) {
    // Flattened list: (segment, page) per row; -1 page = chapter header, -2 = loading / error / end row of a segment.
    val rows = remember(ui.segments) {
        buildList {
            ui.segments.forEachIndexed { s, seg ->
                if (s > 0) add(s to -1)
                when (val p = seg.pages) {
                    is LoadState.Ok -> p.value.indices.forEach { add(s to it) }
                    else -> add(s to -2)
                }
            }
            add(-1 to -3) // end row
        }
    }
    val list = rememberLazyListState(initialFirstVisibleItemIndex = startPage.coerceIn(0, (rows.size - 1).coerceAtLeast(0)))
    val gap = if (reader.mode == ReaderMode.WEBTOON) 0.dp else reader.pageGapDp.dp
    LaunchedEffect(list, rows) {
        snapshotFlow { list.firstVisibleItemIndex }.distinctUntilChanged().collect { i ->
            val (s, p) = rows.getOrNull(i) ?: return@collect
            if (s >= 0 && p >= 0) onPage(s, p)
        }
    }
    val atEnd by remember(rows) { derivedStateOf { (list.layoutInfo.visibleItemsInfo.lastOrNull()?.index ?: 0) >= rows.size - 2 } }
    LaunchedEffect(atEnd, reader.infiniteScroll) { if (atEnd && reader.infiniteScroll) onAppendNext() }
    LaunchedEffect(step) {
        if (step == 0) return@LaunchedEffect
        val viewport = list.layoutInfo.viewportSize.height.toFloat()
        list.animateScrollBy(viewport * 0.85f * step)
        onStepConsumed()
    }
    LaunchedEffect(jumpTo) {
        val j = jumpTo ?: return@LaunchedEffect
        val index = rows.indexOfFirst { it.first == ui.current && it.second == j }
        if (index >= 0) list.scrollToItem(index)
        onJumpConsumed()
    }
    // Auto-scroll (checklist manga.reader.behavior): px per second from the settings.
    LaunchedEffect(autoScroll, reader.autoScrollSpeed) {
        if (!autoScroll) return@LaunchedEffect
        var last = withFrameNanos { it }
        while (true) {
            val now = withFrameNanos { it }
            val dt = (now - last) / 1_000_000_000f
            last = now
            list.scrollBy(reader.autoScrollSpeed * dt)
        }
    }
    LazyColumn(
        state = list,
        verticalArrangement = Arrangement.spacedBy(gap),
        horizontalAlignment = Alignment.CenterHorizontally,
        modifier = Modifier.fillMaxSize(),
    ) {
        itemsIndexed(rows, key = { i, r -> "${r.first}:${r.second}:$i" }) { _, (s, p) ->
            when {
                s == -1 -> EndCard(hasNext = hasNext, onNext = onNext, color = textColor, infinite = reader.infiniteScroll)
                p == -1 -> {
                    val c = ui.segments[s].chapter
                    Text(
                        ChapterText.formatNumber(c.number)?.let { stringResource(R.string.manga_chapter_n, it) } ?: (c.title ?: c.id),
                        color = textColor,
                        style = MaterialTheme.typography.titleMedium,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.fillMaxWidth().padding(vertical = 32.dp),
                    )
                }
                p == -2 -> when (val st = ui.segments[s].pages) {
                    is LoadState.Err -> PagesError(st.cause.userMessage(), onRetry = onAppendNext, onSwitch = onSwitch, color = textColor, modifier = Modifier.padding(32.dp))
                    else -> Box(Modifier.fillMaxWidth().height(240.dp), contentAlignment = Alignment.Center) { CircularProgressIndicator() }
                }
                else -> {
                    val page = (ui.segments[s].pages as LoadState.Ok).value[p]
                    PageImage(page, p, reader, inList = true, failed = page.url in ui.failedPages, onError = onError, onLoaded = onLoaded, onSwitch = onSwitch, textColor = textColor)
                }
            }
        }
    }
}

@Composable
private fun PageImage(
    page: ChapterPage,
    index: Int,
    reader: ReaderSettings,
    inList: Boolean,
    failed: Boolean,
    onError: (String) -> Unit,
    onLoaded: (String) -> Unit,
    onSwitch: () -> Unit,
    textColor: Color,
) {
    val context = LocalContext.current
    var retry by remember(page.url) { mutableIntStateOf(0) }
    var loaded by remember(page.url, retry) { mutableStateOf(false) }
    var error by remember(page.url, retry) { mutableStateOf(false) }
    val request = remember(page.url, retry, reader.cropBorders) { pageRequest(context, page, reader.cropBorders, retry) }
    val fit = if (reader.mode == ReaderMode.WEBTOON) ReaderFit.WIDTH else reader.fit
    val maxWidth = if (reader.maxWidthDp > 0) Modifier.widthIn(max = reader.maxWidthDp.dp) else Modifier
    val sizing = when {
        inList && (fit == ReaderFit.WIDTH || fit == ReaderFit.BOTH) -> Modifier.fillMaxWidth()
        inList -> Modifier.wrapContentSize()
        fit == ReaderFit.WIDTH -> Modifier.fillMaxWidth().verticalScroll(rememberScrollState())
        fit == ReaderFit.HEIGHT -> Modifier.fillMaxHeight().horizontalScroll(rememberScrollState())
        fit == ReaderFit.BOTH -> Modifier.fillMaxSize()
        else -> Modifier.wrapContentSize()
    }
    val scale = when (fit) {
        ReaderFit.WIDTH -> ContentScale.FillWidth
        ReaderFit.HEIGHT -> ContentScale.FillHeight
        ReaderFit.BOTH -> ContentScale.Fit
        ReaderFit.NATURAL -> ContentScale.None
    }
    Box(maxWidth.then(if (inList && !loaded) Modifier.fillMaxWidth().heightIn(min = 320.dp) else Modifier), contentAlignment = Alignment.Center) {
        if (error || failed && !loaded) {
            Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.padding(24.dp)) {
                Text(stringResource(R.string.manga_reader_page_failed, index + 1), color = textColor)
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(top = 8.dp)) {
                    OutlinedButton(onClick = { retry++ }) { Text(stringResource(UiR.string.ui_retry)) }
                    Button(onClick = onSwitch) { Text(stringResource(R.string.manga_reader_switch_source)) }
                }
            }
        } else {
            AsyncImage(
                model = request,
                contentDescription = stringResource(R.string.manga_reader_page_n, index + 1),
                contentScale = scale,
                modifier = sizing,
                onState = { state ->
                    when (state) {
                        is AsyncImagePainter.State.Success -> {
                            loaded = true
                            onLoaded(page.url)
                        }
                        is AsyncImagePainter.State.Error -> {
                            error = true
                            onError(page.url)
                        }
                        else -> Unit
                    }
                },
            )
            if (!loaded) CircularProgressIndicator(Modifier.size(32.dp))
        }
    }
}

@Composable
private fun PagesError(message: String, onRetry: () -> Unit, onSwitch: () -> Unit, color: Color, modifier: Modifier = Modifier) {
    Column(modifier.padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(stringResource(R.string.manga_reader_pages_failed), style = MaterialTheme.typography.titleMedium, color = color)
        Text(message, style = MaterialTheme.typography.bodySmall, color = color, textAlign = TextAlign.Center)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            OutlinedButton(onClick = onRetry) { Text(stringResource(UiR.string.ui_retry)) }
            Button(onClick = onSwitch) { Text(stringResource(R.string.manga_reader_switch_source)) }
        }
    }
}

@Composable
private fun EndCard(hasNext: Boolean, onNext: () -> Unit, color: Color, infinite: Boolean = false) {
    Column(Modifier.fillMaxWidth().padding(vertical = 48.dp, horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        Text(stringResource(R.string.manga_reader_chapter_end), color = color, style = MaterialTheme.typography.titleMedium)
        if (hasNext) {
            if (!infinite) Button(onClick = onNext, modifier = Modifier.padding(top = 12.dp)) { Text(stringResource(R.string.manga_reader_next_chapter)) }
        } else {
            Text(stringResource(R.string.manga_reader_last_chapter), color = color, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(top = 8.dp))
        }
    }
}

@Composable
private fun TopChrome(
    title: String,
    chapterLine: String,
    sourceName: String?,
    fullscreen: Boolean,
    onBack: () -> Unit,
    onFullscreen: () -> Unit,
    onSettings: () -> Unit,
    onChapters: () -> Unit,
    onSources: () -> Unit,
    onCapture: () -> Unit,
    onOpenSettings: () -> Unit,
) {
    var menu by remember { mutableStateOf(false) }
    Surface(color = MaterialTheme.colorScheme.surfaceContainer.copy(alpha = 0.94f), modifier = Modifier.fillMaxWidth()) {
        Row(Modifier.statusBarsPadding().padding(horizontal = 4.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(UiR.string.ui_back)) }
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(listOfNotNull(chapterLine.ifBlank { null }, sourceName).joinToString(" · "), style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            IconButton(onClick = onChapters) { Icon(Icons.AutoMirrored.Outlined.List, contentDescription = stringResource(R.string.manga_reader_chapters)) }
            IconButton(onClick = onSettings) { Icon(Icons.Outlined.Settings, contentDescription = stringResource(R.string.manga_reader_settings)) }
            Box {
                IconButton(onClick = { menu = true }) { Icon(Icons.Outlined.MoreVert, contentDescription = stringResource(R.string.manga_more_options)) }
                DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                    DropdownMenuItem(
                        text = { Text(stringResource(if (fullscreen) R.string.manga_reader_exit_fullscreen else R.string.manga_reader_fullscreen)) },
                        leadingIcon = { Icon(if (fullscreen) Icons.Outlined.FullscreenExit else Icons.Outlined.Fullscreen, contentDescription = null) },
                        onClick = { menu = false; onFullscreen() },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.manga_reader_switch_source)) },
                        leadingIcon = { Icon(Icons.Outlined.SwapHoriz, contentDescription = null) },
                        onClick = { menu = false; onSources() },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.manga_reader_capture)) },
                        leadingIcon = { Icon(Icons.Outlined.CameraAlt, contentDescription = null) },
                        onClick = { menu = false; onCapture() },
                    )
                    DropdownMenuItem(
                        text = { Text(stringResource(R.string.manga_reader_all_settings)) },
                        leadingIcon = { Icon(Icons.Outlined.Settings, contentDescription = null) },
                        onClick = { menu = false; onOpenSettings() },
                    )
                }
            }
        }
    }
}

@Composable
private fun BottomChrome(
    ui: MangaReaderUi,
    reader: ReaderSettings,
    hasPrevious: Boolean,
    hasNext: Boolean,
    autoScroll: Boolean,
    onAutoScroll: () -> Unit,
    onPrevious: () -> Unit,
    onNext: () -> Unit,
    onJump: (Int) -> Unit,
    onRail: () -> Unit,
) {
    val total = ui.pages.size
    Surface(color = MaterialTheme.colorScheme.surfaceContainer.copy(alpha = 0.94f), modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.navigationBarsPadding().padding(horizontal = 8.dp, vertical = 4.dp)) {
            if (total > 1) {
                var drag by remember(ui.page, total) { mutableFloatStateOf(ui.page.toFloat()) }
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("${drag.toInt() + 1}", style = MaterialTheme.typography.labelMedium, modifier = Modifier.width(36.dp), textAlign = TextAlign.Center)
                    Slider(
                        value = drag,
                        onValueChange = { drag = it },
                        onValueChangeFinished = { onJump(drag.toInt()) },
                        valueRange = 0f..(total - 1).toFloat(),
                        modifier = Modifier.weight(1f).graphicsLayer { if (reader.mode == ReaderMode.PAGED && reader.direction == ReaderDirection.RTL) scaleX = -1f },
                    )
                    Text("$total", style = MaterialTheme.typography.labelMedium, modifier = Modifier.width(36.dp), textAlign = TextAlign.Center)
                }
            } else if (total == 0) {
                LinearProgressIndicator(Modifier.fillMaxWidth().padding(8.dp))
            }
            Row(verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onPrevious, enabled = hasPrevious) { Icon(Icons.Outlined.SkipPrevious, contentDescription = stringResource(R.string.manga_reader_previous_chapter)) }
                IconButton(onClick = onRail) { Icon(Icons.Outlined.ViewCarousel, contentDescription = stringResource(R.string.manga_reader_pages)) }
                if (reader.mode != ReaderMode.PAGED && reader.autoScrollSpeed > 0) {
                    IconButton(onClick = onAutoScroll) {
                        Icon(if (autoScroll) Icons.Outlined.Pause else Icons.Outlined.PlayArrow, contentDescription = stringResource(R.string.manga_reader_auto_scroll))
                    }
                }
                Text(
                    if (total > 0) stringResource(R.string.manga_reader_page_of, ui.page + 1, total) else "",
                    style = MaterialTheme.typography.labelLarge,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = onNext, enabled = hasNext) { Icon(Icons.Outlined.SkipNext, contentDescription = stringResource(R.string.manga_reader_next_chapter)) }
            }
        }
    }
}
