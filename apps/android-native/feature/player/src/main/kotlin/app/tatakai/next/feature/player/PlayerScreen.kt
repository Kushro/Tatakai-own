// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import android.app.Activity
import android.app.PictureInPictureParams
import android.content.Context
import android.content.ContextWrapper
import android.content.pm.ActivityInfo
import android.media.AudioManager
import android.os.Build
import android.util.Rational
import androidx.activity.ComponentActivity
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.horizontalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.BrightnessMedium
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Dns
import androidx.compose.material.icons.rounded.Forward10
import androidx.compose.material.icons.rounded.HighQuality
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PictureInPictureAlt
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Replay10
import androidx.compose.material.icons.rounded.Speed
import androidx.compose.material.icons.rounded.Subtitles
import androidx.compose.material3.Button
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.core.app.PictureInPictureModeChangedInfo
import androidx.core.util.Consumer
import androidx.core.view.WindowCompat
import androidx.core.view.WindowInsetsCompat
import androidx.core.view.WindowInsetsControllerCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LifecycleEventEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.compose.ContentFrame
import app.tatakai.next.model.PlayerArgs
import app.tatakai.next.player.HistoryFlush
import app.tatakai.next.player.PlaybackController
import kotlinx.coroutines.delay
import org.koin.compose.getKoin
import java.util.Locale
import kotlin.math.abs
import kotlin.math.roundToInt

private enum class Sheet { Quality, Source, Subtitles, Speed }

private val SPEEDS = listOf(0.5f, 0.75f, 1f, 1.25f, 1.5f, 2f)

/**
 * Full-screen player. [args] with `remoteSessionId` plays the HLS of that Link session through the mTLS client of the pairing;
 * otherwise sources come from the installed extensions. Progress goes to the history (E9 keys) and the screen survives rotation.
 * The host Activity needs `android:supportsPictureInPicture="true"` and `configChanges` for orientation|screenSize for PiP to work.
 */
@OptIn(UnstableApi::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
fun PlayerScreen(args: PlayerArgs, onBack: () -> Unit) {
    val koin = getKoin()
    val vm: PlayerViewModel = viewModel(
        key = "player-${args.media.anilistId}-${args.media.malId}-${args.episode}-${args.remoteSessionId}",
        factory = viewModelFactory {
            initializer {
                PlayerViewModel(
                    args = args,
                    metadata = koin.get(),
                    extensions = koin.get(),
                    history = koin.get(),
                    link = koin.get(),
                    remote = koin.get(),
                    factory = koin.get(),
                    flush = koin.getOrNull<HistoryFlush>(),
                )
            }
        },
    )
    val state by vm.state.collectAsStateWithLifecycle()
    val title = if (args.remoteSessionId != null) args.title else stringResource(R.string.player_episode_title, args.title, args.episode)

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        when (val s = state) {
            is PlayerUiState.ResolvingSources -> Centered { LoadingIndicator(Modifier.size(56.dp)); Text(stringResource(R.string.player_loading_sources), color = Color.White) }
            is PlayerUiState.NoSources -> Centered { Message(R.string.player_no_sources); RetryButton(vm::retry) }
            is PlayerUiState.RemoteUnavailable -> Centered { Message(R.string.player_remote_unavailable) }
            is PlayerUiState.Failed -> Centered { Message(R.string.player_error); RetryButton(vm::retry) }
            is PlayerUiState.Ready -> PlayerContent(s.controller, s.sourceLabels, title, onBack)
        }
        // Back is always reachable, also while sources load or fail.
        if (state !is PlayerUiState.Ready) {
            IconButton(onClick = onBack, Modifier.statusBarsPadding().padding(8.dp).align(Alignment.TopStart)) {
                Icon(Icons.AutoMirrored.Rounded.ArrowBack, stringResource(R.string.player_back), tint = Color.White)
            }
        }
    }
    BackHandlerCompat(onBack, enabled = true)
}

@Composable
private fun BackHandlerCompat(onBack: () -> Unit, enabled: Boolean) {
    androidx.activity.compose.BackHandler(enabled = enabled, onBack = onBack)
}

@Composable
private fun Centered(content: @Composable () -> Unit) {
    Column(Modifier.fillMaxSize().padding(24.dp), Arrangement.spacedBy(16.dp, Alignment.CenterVertically), Alignment.CenterHorizontally) { content() }
}

@Composable
private fun Message(res: Int) {
    Text(stringResource(res), color = Color.White, textAlign = TextAlign.Center, style = MaterialTheme.typography.bodyLarge)
}

@Composable
private fun RetryButton(onClick: () -> Unit) {
    Button(onClick = onClick) { Text(stringResource(R.string.player_retry)) }
}

@OptIn(UnstableApi::class, ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
private fun PlayerContent(controller: PlaybackController, sourceLabels: List<String>, title: String, onBack: () -> Unit) {
    val state by controller.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val view = LocalView.current
    val activity = remember(context) { context.findActivity() }

    var controlsVisible by remember { mutableStateOf(true) }
    var interaction by remember { mutableIntStateOf(0) }
    var sheet by remember { mutableStateOf<Sheet?>(null) }
    var inPip by remember { mutableStateOf(false) }
    var hud by remember { mutableStateOf<Hud?>(null) }

    // Immersive landscape playback; everything is restored on exit.
    DisposableEffect(activity) {
        val window = activity?.window
        val insets = window?.let { WindowCompat.getInsetsController(it, view) }
        val previousOrientation = activity?.requestedOrientation
        val previousBrightness = window?.attributes?.screenBrightness
        view.keepScreenOn = true
        insets?.systemBarsBehavior = WindowInsetsControllerCompat.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE
        insets?.hide(WindowInsetsCompat.Type.systemBars())
        activity?.requestedOrientation = ActivityInfo.SCREEN_ORIENTATION_SENSOR_LANDSCAPE
        onDispose {
            view.keepScreenOn = false
            insets?.show(WindowInsetsCompat.Type.systemBars())
            previousOrientation?.let { activity.requestedOrientation = it }
            if (window != null && previousBrightness != null) {
                window.attributes = window.attributes.also { it.screenBrightness = previousBrightness }
            }
        }
    }

    // Picture in picture state (the controller keeps playing; only the chrome is hidden).
    DisposableEffect(activity) {
        val host = activity as? ComponentActivity
        val listener = Consumer<PictureInPictureModeChangedInfo> { inPip = it.isInPictureInPictureMode }
        host?.addOnPictureInPictureModeChangedListener(listener)
        onDispose { host?.removeOnPictureInPictureModeChangedListener(listener) }
    }
    // Android 12+: leave to PiP automatically while playing.
    LaunchedEffect(state.isPlaying, activity) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            activity?.setPictureInPictureParams(pipParams(autoEnter = state.isPlaying))
        }
    }
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { if (!inPip) controller.pause() }

    LaunchedEffect(controlsVisible, interaction, state.isPlaying, sheet) {
        if (controlsVisible && state.isPlaying && sheet == null) {
            delay(CONTROLS_TIMEOUT_MS)
            controlsVisible = false
        }
    }
    LaunchedEffect(hud) {
        if (hud != null) {
            delay(HUD_TIMEOUT_MS)
            hud = null
        }
    }

    Box(Modifier.fillMaxSize().background(Color.Black)) {
        ContentFrame(player = controller.player, modifier = Modifier.fillMaxSize(), contentScale = ContentScale.Fit)

        if (!inPip) {
            GestureLayer(
                controller = controller,
                state = state,
                onToggleControls = { controlsVisible = !controlsVisible },
                onHud = { hud = it },
            )

            if (state.isBuffering && state.error == null) {
                LoadingIndicator(Modifier.align(Alignment.Center).size(56.dp))
            }
            if (state.fallbackCount > 0 && state.error == null && state.isBuffering) {
                Text(
                    stringResource(R.string.player_fallback_notice, state.fallbackCount, PlaybackController.MAX_FALLBACKS),
                    color = Color.White,
                    style = MaterialTheme.typography.labelLarge,
                    modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = 96.dp),
                )
            }
            state.error?.let {
                Centered {
                    Message(R.string.player_error)
                    RetryButton { controller.selectSource(if (state.sourceCount > 0) (state.sourceIndex + 1) % state.sourceCount else 0) }
                }
            }
            hud?.let { HudPill(it, Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 16.dp)) }

            AnimatedVisibility(controlsVisible, enter = fadeIn(), exit = fadeOut(), modifier = Modifier.fillMaxSize()) {
                Controls(
                    controller = controller,
                    state = state,
                    title = title,
                    onBack = onBack,
                    onPip = { activity?.let { enterPip(it) } },
                    onOpenSheet = { sheet = it; interaction++ },
                    onInteract = { interaction++ },
                )
            }
        }
    }

    sheet?.let { kind ->
        ModalBottomSheet(onDismissRequest = { sheet = null }) {
            when (kind) {
                Sheet.Quality -> {
                    SheetRow(stringResource(R.string.player_quality_auto), state.selectedHeight == null) { controller.selectVideoHeight(null); sheet = null }
                    state.videoHeights.forEach { h ->
                        SheetRow(stringResource(R.string.player_quality_height, h), state.selectedHeight == h) { controller.selectVideoHeight(h); sheet = null }
                    }
                }
                Sheet.Source -> sourceLabels.forEachIndexed { i, label ->
                    SheetRow(label.ifBlank { "#${i + 1}" }, i == state.sourceIndex) { controller.selectSource(i); sheet = null }
                }
                Sheet.Subtitles -> {
                    SheetRow(stringResource(R.string.player_subtitles_off), !state.subtitlesEnabled) { controller.selectSubtitle(null); sheet = null }
                    state.subtitles.forEach { o ->
                        SheetRow(o.label, o.selected) { controller.selectSubtitle(o.groupIndex); sheet = null }
                    }
                }
                Sheet.Speed -> SPEEDS.forEach { sp ->
                    SheetRow(stringResource(R.string.player_speed_value, speedText(sp)), state.speed == sp) { controller.setSpeed(sp); sheet = null }
                }
            }
            Spacer(Modifier.navigationBarsPadding().size(width = 1.dp, height = 16.dp))
        }
    }
}


@Composable
private fun SheetRow(text: String, selected: Boolean, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(text, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        trailingContent = { if (selected) Icon(Icons.Rounded.Check, contentDescription = null) },
        modifier = Modifier.clickable(onClick = onClick),
    )
}

private data class Hud(val kind: Kind, val percent: Int = 0, val seconds: Int = 0) {
    enum class Kind { Brightness, Volume, Seek }
}

@Composable
private fun HudPill(hud: Hud, modifier: Modifier) {
    Surface(modifier, shape = RoundedCornerShape(50), color = Color.Black.copy(alpha = 0.7f), contentColor = Color.White) {
        Row(Modifier.padding(horizontal = 16.dp, vertical = 10.dp), Arrangement.spacedBy(8.dp), Alignment.CenterVertically) {
            when (hud.kind) {
                Hud.Kind.Brightness -> {
                    Icon(Icons.Rounded.BrightnessMedium, stringResource(R.string.player_brightness))
                    Text(stringResource(R.string.player_percent, hud.percent))
                }
                Hud.Kind.Volume -> {
                    Icon(Icons.AutoMirrored.Rounded.VolumeUp, stringResource(R.string.player_volume))
                    Text(stringResource(R.string.player_percent, hud.percent))
                }
                Hud.Kind.Seek -> Text(stringResource(R.string.player_seek_seconds, hud.seconds))
            }
        }
    }
}

/**
 * Tap toggles the controls, double tap on either half seeks 10 s, horizontal drag scrubs, vertical drag on the left half changes the
 * brightness and on the right half the volume.
 */
@OptIn(UnstableApi::class)
@Composable
private fun GestureLayer(
    controller: PlaybackController,
    state: app.tatakai.next.player.PlaybackState,
    onToggleControls: () -> Unit,
    onHud: (Hud) -> Unit,
) {
    val context = LocalContext.current
    val activity = remember(context) { context.findActivity() }
    val audio = remember(context) { context.getSystemService(Context.AUDIO_SERVICE) as AudioManager }
    var brightness by remember {
        val current = activity?.window?.attributes?.screenBrightness ?: -1f
        mutableFloatStateOf(if (current >= 0f) current else 0.5f)
    }
    var volume by remember {
        val max = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC).coerceAtLeast(1)
        mutableFloatStateOf(audio.getStreamVolume(AudioManager.STREAM_MUSIC).toFloat() / max)
    }
    var onLeft by remember { mutableStateOf(true) }
    var scrubStartMs by remember { mutableLongStateOf(0L) }
    var scrubDeltaMs by remember { mutableLongStateOf(0L) }

    Box(
        Modifier
            .fillMaxSize()
            .pointerInput(Unit) {
                detectTapGestures(
                    onTap = { onToggleControls() },
                    onDoubleTap = { offset ->
                        val back = offset.x < size.width / 2f
                        controller.seekBy(if (back) -SEEK_STEP_MS else SEEK_STEP_MS)
                        onHud(Hud(Hud.Kind.Seek, seconds = if (back) -10 else 10))
                    },
                )
            }
            .pointerInput(Unit) {
                detectVerticalDragGestures(
                    onDragStart = { onLeft = it.x < size.width / 2f },
                    onVerticalDrag = { change, dy ->
                        change.consume()
                        val delta = -dy / size.height * 1.2f
                        if (onLeft) {
                            brightness = (brightness + delta).coerceIn(0.01f, 1f)
                            activity?.window?.let { w -> w.attributes = w.attributes.also { it.screenBrightness = brightness } }
                            onHud(Hud(Hud.Kind.Brightness, percent = (brightness * 100).roundToInt()))
                        } else {
                            volume = (volume + delta).coerceIn(0f, 1f)
                            val max = audio.getStreamMaxVolume(AudioManager.STREAM_MUSIC)
                            audio.setStreamVolume(AudioManager.STREAM_MUSIC, (volume * max).roundToInt(), 0)
                            onHud(Hud(Hud.Kind.Volume, percent = (volume * 100).roundToInt()))
                        }
                    },
                )
            }
            .pointerInput(Unit) {
                detectHorizontalDragGestures(
                    onDragStart = { scrubStartMs = controller.player.currentPosition; scrubDeltaMs = 0 },
                    onHorizontalDrag = { change, dx ->
                        change.consume()
                        // A full-width drag scrubs 90 s.
                        scrubDeltaMs += (dx / size.width * 90_000f).toLong()
                        onHud(Hud(Hud.Kind.Seek, seconds = (scrubDeltaMs / 1000).toInt()))
                    },
                    onDragEnd = { if (abs(scrubDeltaMs) > 500) controller.seekTo(scrubStartMs + scrubDeltaMs) },
                )
            },
    )
}

@OptIn(UnstableApi::class)
@Composable
private fun Controls(
    controller: PlaybackController,
    state: app.tatakai.next.player.PlaybackState,
    title: String,
    onBack: () -> Unit,
    onPip: () -> Unit,
    onOpenSheet: (Sheet) -> Unit,
    onInteract: () -> Unit,
) {
    val scrim = Color.Black.copy(alpha = 0.45f)
    Box(Modifier.fillMaxSize().background(scrim)) {
        // Top bar
        Row(
            Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 8.dp, vertical = 4.dp).align(Alignment.TopCenter),
            Arrangement.spacedBy(4.dp),
            Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Rounded.ArrowBack, stringResource(R.string.player_back), tint = Color.White) }
            Text(title, Modifier.weight(1f), color = Color.White, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.titleMedium)
            IconButton(onClick = onPip) { Icon(Icons.Rounded.PictureInPictureAlt, stringResource(R.string.player_pip), tint = Color.White) }
        }

        // Transport
        Row(Modifier.align(Alignment.Center), Arrangement.spacedBy(24.dp), Alignment.CenterVertically) {
            FilledTonalIconButton(onClick = { controller.seekBy(-SEEK_STEP_MS); onInteract() }, Modifier.size(56.dp)) {
                Icon(Icons.Rounded.Replay10, stringResource(R.string.player_seek_back))
            }
            FilledIconButton(onClick = { controller.togglePlay(); onInteract() }, Modifier.size(80.dp)) {
                Icon(
                    if (state.isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                    stringResource(if (state.isPlaying) R.string.player_pause else R.string.player_play),
                    Modifier.size(40.dp),
                )
            }
            FilledTonalIconButton(onClick = { controller.seekBy(SEEK_STEP_MS); onInteract() }, Modifier.size(56.dp)) {
                Icon(Icons.Rounded.Forward10, stringResource(R.string.player_seek_forward))
            }
        }

        // Seek bar + chips
        Column(Modifier.fillMaxWidth().align(Alignment.BottomCenter).navigationBarsPadding().padding(horizontal = 16.dp, vertical = 8.dp)) {
            SeekBar(controller, state, onInteract)
            Row(Modifier.horizontalScroll(rememberScrollState()), Arrangement.spacedBy(8.dp)) {
                if (state.videoHeights.isNotEmpty()) {
                    FilterChip(
                        selected = state.selectedHeight != null,
                        onClick = { onOpenSheet(Sheet.Quality) },
                        label = { Text(state.selectedHeight?.let { stringResource(R.string.player_quality_height, it) } ?: stringResource(R.string.player_quality)) },
                        leadingIcon = { Icon(Icons.Rounded.HighQuality, null, Modifier.size(18.dp)) },
                    )
                }
                if (state.sourceCount > 1) {
                    FilterChip(
                        selected = false,
                        onClick = { onOpenSheet(Sheet.Source) },
                        label = { Text(stringResource(R.string.player_source) + " ${state.sourceIndex + 1}/${state.sourceCount}") },
                        leadingIcon = { Icon(Icons.Rounded.Dns, null, Modifier.size(18.dp)) },
                    )
                }
                if (state.subtitles.isNotEmpty()) {
                    FilterChip(
                        selected = state.subtitlesEnabled,
                        onClick = { onOpenSheet(Sheet.Subtitles) },
                        label = { Text(stringResource(R.string.player_subtitles)) },
                        leadingIcon = { Icon(Icons.Rounded.Subtitles, null, Modifier.size(18.dp)) },
                    )
                }
                FilterChip(
                    selected = state.speed != 1f,
                    onClick = { onOpenSheet(Sheet.Speed) },
                    label = { Text(stringResource(R.string.player_speed_value, speedText(state.speed))) },
                    leadingIcon = { Icon(Icons.Rounded.Speed, null, Modifier.size(18.dp)) },
                )
            }
        }
    }
}

@Composable
private fun SeekBar(controller: PlaybackController, state: app.tatakai.next.player.PlaybackState, onInteract: () -> Unit) {
    var dragging by remember { mutableStateOf(false) }
    var dragValue by remember { mutableFloatStateOf(0f) }
    val duration = state.durationMs.coerceAtLeast(1)
    val shown = if (dragging) dragValue else state.positionMs.toFloat().coerceIn(0f, duration.toFloat())
    Column {
        Slider(
            value = shown,
            onValueChange = { dragging = true; dragValue = it; onInteract() },
            onValueChangeFinished = { controller.seekTo(dragValue.toLong()); dragging = false },
            valueRange = 0f..duration.toFloat(),
            enabled = state.durationMs > 0,
        )
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp), Arrangement.SpaceBetween) {
            Text(formatTime(shown.toLong()), color = Color.White, style = MaterialTheme.typography.labelMedium)
            Text(if (state.durationMs > 0) formatTime(state.durationMs) else "--:--", color = Color.White.copy(alpha = 0.8f), style = MaterialTheme.typography.labelMedium)
        }
    }
}

private fun formatTime(ms: Long): String {
    val total = (ms / 1000).coerceAtLeast(0)
    val h = total / 3600
    val m = (total % 3600) / 60
    val s = total % 60
    return if (h > 0) String.format(Locale.ROOT, "%d:%02d:%02d", h, m, s) else String.format(Locale.ROOT, "%d:%02d", m, s)
}

private fun speedText(speed: Float): String = if (speed % 1f == 0f) speed.toInt().toString() else speed.toString().trimEnd('0')

private fun pipParams(autoEnter: Boolean): PictureInPictureParams =
    PictureInPictureParams.Builder().setAspectRatio(Rational(16, 9)).apply {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) setAutoEnterEnabled(autoEnter)
    }.build()

private fun enterPip(activity: Activity) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) activity.enterPictureInPictureMode(pipParams(autoEnter = false))
}

private tailrec fun Context.findActivity(): Activity? =
    when (this) {
        is Activity -> this
        is ContextWrapper -> baseContext.findActivity()
        else -> null
    }

private const val CONTROLS_TIMEOUT_MS = 4_000L
private const val HUD_TIMEOUT_MS = 900L
private const val SEEK_STEP_MS = 10_000L
