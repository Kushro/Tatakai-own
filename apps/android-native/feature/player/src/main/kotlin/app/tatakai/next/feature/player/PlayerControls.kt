// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(UnstableApi::class)

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
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.automirrored.rounded.VolumeUp
import androidx.compose.material.icons.rounded.Audiotrack
import androidx.compose.material.icons.rounded.BrightnessMedium
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Download
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.Forward10
import androidx.compose.material.icons.rounded.HighQuality
import androidx.compose.material.icons.rounded.Info
import androidx.compose.material.icons.rounded.Language
import androidx.compose.material.icons.rounded.Pause
import androidx.compose.material.icons.rounded.PictureInPictureAlt
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Replay10
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material.icons.rounded.Speed
import androidx.compose.material.icons.rounded.Subtitles
import androidx.compose.material3.AssistChip
import androidx.compose.material3.AssistChipDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Slider
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
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
import androidx.media3.common.util.UnstableApi
import androidx.media3.ui.compose.ContentFrame
import app.tatakai.next.model.SkipKind
import app.tatakai.next.model.SourceKind
import app.tatakai.next.player.PlaybackController
import app.tatakai.next.player.PlaybackState
import app.tatakai.next.player.PlayerSession
import app.tatakai.next.player.mpv.MpvPlayerSession
import app.tatakai.next.player.mpv.MpvPlayerSurface
import app.tatakai.next.player.rangeSpans
import app.tatakai.next.player.readySpan
import app.tatakai.next.ui.format.formatDuration
import kotlinx.coroutines.delay
import kotlin.math.abs
import kotlin.math.roundToInt

private enum class Sheet { Quality, Subtitles, Audio, Speed }

private val SPEEDS = listOf(0.5f, 0.75f, 1f, 1.25f, 1.5f, 2f)

/** The running engine: video surface, subtitles (Media3), gestures, controls, overlays, PiP. */
@OptIn(UnstableApi::class, ExperimentalMaterial3Api::class, ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun PlayerContent(
    vm: PlayerViewModel,
    session: PlayerSession,
    state: PlayerUiState,
    title: String,
    flash: PlayerNotice?,
    onFlashDone: () -> Unit,
    onBack: () -> Unit,
    onOpenPicker: () -> Unit,
    onOpenTorrent: (String) -> Unit,
) {
    val ps by session.state.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val view = LocalView.current
    val activity = remember(context) { context.findActivity() }
    val scheme = MaterialTheme.colorScheme

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

    // Picture in picture state (the session keeps playing; only the chrome is hidden).
    DisposableEffect(activity) {
        val host = activity as? ComponentActivity
        val listener = Consumer<PictureInPictureModeChangedInfo> { inPip = it.isInPictureInPictureMode }
        host?.addOnPictureInPictureModeChangedListener(listener)
        onDispose { host?.removeOnPictureInPictureModeChangedListener(listener) }
    }
    // Android 12+: leave to PiP automatically while playing.
    LaunchedEffect(ps.isPlaying, activity) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            activity?.setPictureInPictureParams(pipParams(autoEnter = ps.isPlaying))
        }
    }
    LifecycleEventEffect(Lifecycle.Event.ON_STOP) { if (!inPip) session.pause() }

    // Controls stay up until the first frame (resolving / buffering), then hide after a few seconds of playback.
    val keepUp = !ps.started || state.phase == PlayerPhase.RESOLVING
    LaunchedEffect(controlsVisible, interaction, ps.isPlaying, sheet, keepUp) {
        if (controlsVisible && ps.isPlaying && sheet == null && !keepUp) {
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

    Box(Modifier.fillMaxSize().background(scheme.background)) {
        when (session) {
            is PlaybackController -> {
                ContentFrame(player = session.player, modifier = Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
                val cues by session.cues.collectAsState()
                val style by session.subtitleStyle.collectAsState()
                SubtitleOverlay(
                    cues,
                    style,
                    Modifier.align(Alignment.BottomCenter).navigationBarsPadding()
                        .padding(bottom = if (controlsVisible && !inPip) 136.dp else 24.dp, start = 24.dp, end = 24.dp),
                )
            }
            is MpvPlayerSession -> MpvPlayerSurface(session.engine, Modifier.fillMaxSize())
        }

        if (!inPip) {
            GestureLayer(
                session = session,
                onToggleControls = { controlsVisible = !controlsVisible },
                onHud = { hud = it },
            )

            val resolving = state.phase == PlayerPhase.RESOLVING
            if ((ps.isBuffering || resolving) && ps.error == null) {
                BufferingOverlay(state, Modifier.align(Alignment.Center))
            }
            hud?.let { HudPill(it, Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 16.dp)) }
            flash?.let { FlashToast(it, onFlashDone, Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 64.dp)) }

            val skip = state.markers.skipAt(ps.positionMs / 1000.0)
            if (skip != null && ps.started) {
                SkipButton(
                    kind = skip.first,
                    onClick = { vm.seekBySkip(); interaction++ },
                    modifier = Modifier.align(Alignment.BottomEnd).navigationBarsPadding().padding(end = 24.dp, bottom = 140.dp),
                )
            }
            state.next?.let { next ->
                NextEpisodeCard(
                    next = next,
                    onPlayNow = vm::playNext,
                    onCancel = vm::cancelNext,
                    modifier = Modifier.align(Alignment.BottomEnd).navigationBarsPadding().padding(end = 24.dp, bottom = 140.dp),
                )
            }

            AnimatedVisibility(controlsVisible || keepUp, enter = fadeIn(), exit = fadeOut(), modifier = Modifier.fillMaxSize()) {
                Controls(
                    session = session,
                    ps = ps,
                    state = state,
                    title = title,
                    onBack = onBack,
                    onPip = { activity?.let { enterPip(it) } },
                    onOpenSheet = { sheet = it; interaction++ },
                    onOpenPicker = onOpenPicker,
                    onOpenTorrent = onOpenTorrent,
                    onNext = vm::playNext,
                    onInteract = { interaction++ },
                )
            }
        }
    }

    sheet?.let { kind ->
        ModalBottomSheet(onDismissRequest = { sheet = null }) {
            when (kind) {
                Sheet.Quality -> {
                    SheetRow(stringResource(R.string.player_quality_auto), ps.selectedHeight == null) { session.selectVideoHeight(null); sheet = null }
                    ps.videoHeights.forEach { h ->
                        SheetRow(stringResource(R.string.player_quality_height, h), ps.selectedHeight == h) { session.selectVideoHeight(h); sheet = null }
                    }
                }
                Sheet.Subtitles -> {
                    if (!ps.fullAss && ps.subtitles.any { it.ass }) {
                        ListItem(
                            headlineContent = { Text(stringResource(R.string.player_ass_simplified), style = MaterialTheme.typography.bodySmall) },
                            leadingContent = { Icon(Icons.Rounded.Info, contentDescription = null) },
                        )
                    }
                    SheetRow(stringResource(R.string.player_subtitles_off), !ps.subtitlesEnabled) { session.selectSubtitle(null); sheet = null }
                    ps.subtitles.forEach { o ->
                        SheetRow(o.label, o.selected) { session.selectSubtitle(o.groupIndex); sheet = null }
                    }
                }
                Sheet.Audio -> ps.audioTracks.forEach { a ->
                    SheetRow(a.label, a.selected) { session.selectAudio(a.id); sheet = null }
                }
                Sheet.Speed -> SPEEDS.forEach { sp ->
                    SheetRow(stringResource(R.string.player_speed_value, speedText(sp)), ps.speed == sp) { session.setSpeed(sp); sheet = null }
                }
            }
            Spacer(Modifier.navigationBarsPadding().height(16.dp))
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
    val scheme = MaterialTheme.colorScheme
    Surface(modifier, shape = RoundedCornerShape(50), color = scheme.surfaceContainerHigh.copy(alpha = 0.85f), contentColor = scheme.onSurface) {
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
@Composable
private fun GestureLayer(session: PlayerSession, onToggleControls: () -> Unit, onHud: (Hud) -> Unit) {
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
            .pointerInput(session) {
                detectTapGestures(
                    onTap = { onToggleControls() },
                    onDoubleTap = { offset ->
                        val back = offset.x < size.width / 2f
                        session.seekBy(if (back) -SEEK_STEP_MS else SEEK_STEP_MS)
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
            .pointerInput(session) {
                detectHorizontalDragGestures(
                    onDragStart = { scrubStartMs = session.currentPositionMs(); scrubDeltaMs = 0 },
                    onHorizontalDrag = { change, dx ->
                        change.consume()
                        // A full-width drag scrubs 90 s.
                        scrubDeltaMs += (dx / size.width * 90_000f).toLong()
                        onHud(Hud(Hud.Kind.Seek, seconds = (scrubDeltaMs / 1000).toInt()))
                    },
                    onDragEnd = { if (abs(scrubDeltaMs) > 500) session.seekTo(scrubStartMs + scrubDeltaMs) },
                )
            },
    )
}

@Composable
private fun Controls(
    session: PlayerSession,
    ps: PlaybackState,
    state: PlayerUiState,
    title: String,
    onBack: () -> Unit,
    onPip: () -> Unit,
    onOpenSheet: (Sheet) -> Unit,
    onOpenPicker: () -> Unit,
    onOpenTorrent: (String) -> Unit,
    onNext: () -> Unit,
    onInteract: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val on = scheme.onBackground
    Box(Modifier.fillMaxSize().background(scheme.scrim.copy(alpha = 0.45f))) {
        // Top bar: back, title + source chip, "Open in the client", PiP
        Row(
            Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 8.dp, vertical = 4.dp).align(Alignment.TopCenter),
            Arrangement.spacedBy(4.dp),
            Alignment.CenterVertically,
        ) {
            IconButton(onClick = onBack) { Icon(Icons.AutoMirrored.Rounded.ArrowBack, stringResource(R.string.player_back), tint = on) }
            Column(Modifier.weight(1f)) {
                Text(title, color = on, maxLines = 1, overflow = TextOverflow.Ellipsis, style = MaterialTheme.typography.titleMedium)
                Row(Modifier.horizontalScroll(rememberScrollState()), Arrangement.spacedBy(8.dp), Alignment.CenterVertically) {
                    state.current?.let { cur ->
                        val (n, total) = state.position ?: (1 to 1)
                        AssistChip(
                            onClick = { if (state.canPickSource) onOpenPicker() },
                            label = {
                                Text(
                                    stringResource(R.string.player_source_chip, n, total, "${cur.candidate.extName} · ${cur.candidate.label}"),
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                            },
                            leadingIcon = {
                                Icon(
                                    if (cur.candidate.kind == SourceKind.TORRENT) Icons.Rounded.Download else Icons.Rounded.Language,
                                    contentDescription = null,
                                    modifier = Modifier.size(18.dp),
                                )
                            },
                            trailingIcon = if (state.canPickSource) ({ Icon(Icons.Rounded.ExpandMore, null, Modifier.size(18.dp)) }) else null,
                            colors = AssistChipDefaults.assistChipColors(labelColor = on, leadingIconContentColor = on, trailingIconContentColor = on),
                        )
                    }
                    state.torrent?.takeIf { it.fileIndex != null }?.torrentId?.let { id ->
                        AssistChip(
                            onClick = { onOpenTorrent(id) },
                            label = { Text(stringResource(R.string.player_torrent_open_client)) },
                            leadingIcon = { Icon(Icons.AutoMirrored.Rounded.OpenInNew, contentDescription = null, modifier = Modifier.size(18.dp)) },
                            colors = AssistChipDefaults.assistChipColors(labelColor = on, leadingIconContentColor = on),
                        )
                    }
                    state.torrent?.takeIf { it.fileIndex != null }?.let { t ->
                        Text(torrentLine(t), color = on.copy(alpha = 0.8f), style = MaterialTheme.typography.labelMedium, fontFamily = FontFamily.Monospace)
                    }
                }
            }
            IconButton(onClick = onPip) { Icon(Icons.Rounded.PictureInPictureAlt, stringResource(R.string.player_pip), tint = on) }
        }

        // Transport
        Row(Modifier.align(Alignment.Center), Arrangement.spacedBy(24.dp), Alignment.CenterVertically) {
            FilledTonalIconButton(onClick = { session.seekBy(-SEEK_STEP_MS); onInteract() }, Modifier.size(56.dp)) {
                Icon(Icons.Rounded.Replay10, stringResource(R.string.player_seek_back))
            }
            FilledIconButton(onClick = { session.togglePlay(); onInteract() }, Modifier.size(80.dp), enabled = ps.started) {
                Icon(
                    if (ps.isPlaying) Icons.Rounded.Pause else Icons.Rounded.PlayArrow,
                    stringResource(if (ps.isPlaying) R.string.player_pause else R.string.player_play),
                    Modifier.size(40.dp),
                )
            }
            FilledTonalIconButton(onClick = { session.seekBy(SEEK_STEP_MS); onInteract() }, Modifier.size(56.dp)) {
                Icon(Icons.Rounded.Forward10, stringResource(R.string.player_seek_forward))
            }
        }

        // Seek bar + chips
        Column(Modifier.fillMaxWidth().align(Alignment.BottomCenter).navigationBarsPadding().padding(horizontal = 16.dp, vertical = 8.dp)) {
            SeekBar(session, ps, state, onInteract)
            Row(Modifier.horizontalScroll(rememberScrollState()), Arrangement.spacedBy(8.dp)) {
                if (ps.videoHeights.isNotEmpty()) {
                    FilterChip(
                        selected = ps.selectedHeight != null,
                        onClick = { onOpenSheet(Sheet.Quality) },
                        label = { Text(ps.selectedHeight?.let { stringResource(R.string.player_quality_height, it) } ?: stringResource(R.string.player_quality)) },
                        leadingIcon = { Icon(Icons.Rounded.HighQuality, null, Modifier.size(18.dp)) },
                    )
                }
                if (ps.subtitles.isNotEmpty()) {
                    FilterChip(
                        selected = ps.subtitlesEnabled,
                        onClick = { onOpenSheet(Sheet.Subtitles) },
                        label = { Text(stringResource(R.string.player_subtitles)) },
                        leadingIcon = { Icon(Icons.Rounded.Subtitles, null, Modifier.size(18.dp)) },
                    )
                }
                if (ps.audioTracks.size > 1) {
                    FilterChip(
                        selected = false,
                        onClick = { onOpenSheet(Sheet.Audio) },
                        label = { Text(stringResource(R.string.player_audio)) },
                        leadingIcon = { Icon(Icons.Rounded.Audiotrack, null, Modifier.size(18.dp)) },
                    )
                }
                FilterChip(
                    selected = ps.speed != 1f,
                    onClick = { onOpenSheet(Sheet.Speed) },
                    label = { Text(stringResource(R.string.player_speed_value, speedText(ps.speed))) },
                    leadingIcon = { Icon(Icons.Rounded.Speed, null, Modifier.size(18.dp)) },
                )
                if (state.hasNextEpisode && state.remoteless) {
                    FilterChip(
                        selected = false,
                        onClick = onNext,
                        label = {
                            Text(
                                stringResource(
                                    if (state.torrent?.batch == true) R.string.player_torrent_batch_next else R.string.player_next,
                                ),
                            )
                        },
                        leadingIcon = { Icon(Icons.Rounded.SkipNext, null, Modifier.size(18.dp)) },
                    )
                }
            }
        }
    }
}

/**
 * Slider + a thin strip under it with the torrent's downloaded ranges (secondary), the "ready to play" range from the playhead (primary)
 * and the intro / outro / recap marks (tertiary). Colours come from the scheme only.
 */
@Composable
private fun SeekBar(session: PlayerSession, ps: PlaybackState, state: PlayerUiState, onInteract: () -> Unit) {
    var dragging by remember { mutableStateOf(false) }
    var dragValue by remember { mutableFloatStateOf(0f) }
    val duration = ps.durationMs.coerceAtLeast(1)
    val shown = if (dragging) dragValue else ps.positionMs.toFloat().coerceIn(0f, duration.toFloat())
    val scheme = MaterialTheme.colorScheme
    val marks = state.markers.barMarks(ps.durationMs / 1000.0)
    val ranges = state.torrent?.marks.orEmpty()
    val downloaded = rangeSpans(ranges)
    val ready = if (ps.durationMs > 0) readySpan(ranges, ps.positionMs.toDouble() / ps.durationMs) else null
    val bufferedFraction = if (ps.durationMs > 0) (ps.bufferedMs.toFloat() / ps.durationMs).coerceIn(0f, 1f) else 0f
    val trackColor = scheme.onBackground.copy(alpha = 0.18f)
    val bufferColor = scheme.onBackground.copy(alpha = 0.35f)
    val downloadedColor = scheme.secondary
    val readyColor = scheme.primary
    val markColor = scheme.tertiary
    Column {
        Slider(
            value = shown,
            onValueChange = { dragging = true; dragValue = it; onInteract() },
            onValueChangeFinished = { session.seekTo(dragValue.toLong()); dragging = false },
            valueRange = 0f..duration.toFloat(),
            enabled = ps.durationMs > 0,
        )
        Canvas(Modifier.fillMaxWidth().height(6.dp).padding(horizontal = 10.dp)) {
            val w = size.width
            val h = size.height
            val r = CornerRadius(h / 2, h / 2)
            drawRoundRect(trackColor, Offset.Zero, Size(w, h), r)
            if (downloaded.isEmpty()) {
                drawRoundRect(bufferColor, Offset.Zero, Size(w * bufferedFraction, h), r)
            }
            downloaded.forEach { (left, width) -> drawRoundRect(downloadedColor, Offset(w * left, 0f), Size(w * width, h), r) }
            ready?.let { (left, width) -> drawRoundRect(readyColor, Offset(w * left, 0f), Size(w * width, h), r) }
            marks.forEach { m -> drawRoundRect(markColor.copy(alpha = if (m.kind == SkipKind.INTRO) 0.9f else 0.6f), Offset(w * m.left, 0f), Size(w * m.width, h), r) }
        }
        Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 4.dp), Arrangement.SpaceBetween) {
            Text(formatDuration(shown.toLong() / 1000), color = scheme.onBackground, style = MaterialTheme.typography.labelMedium, fontFamily = FontFamily.Monospace)
            Text(
                if (ps.durationMs > 0) formatDuration(ps.durationMs / 1000) else "--:--",
                color = scheme.onBackground.copy(alpha = 0.8f),
                style = MaterialTheme.typography.labelMedium,
                fontFamily = FontFamily.Monospace,
            )
        }
    }
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
