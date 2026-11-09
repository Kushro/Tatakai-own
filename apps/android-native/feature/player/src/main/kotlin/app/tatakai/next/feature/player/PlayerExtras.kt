// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(UnstableApi::class)

package app.tatakai.next.feature.player

import android.app.Activity
import android.content.Context
import android.graphics.Bitmap
import android.media.AudioManager
import android.os.Handler
import android.os.Looper
import android.view.PixelCopy
import android.view.SurfaceView
import android.view.View
import android.view.ViewGroup
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Bedtime
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.Lock
import androidx.compose.material.icons.rounded.PlayArrow
import androidx.compose.material.icons.rounded.Remove
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Restore
import androidx.compose.material.icons.rounded.Translate
import androidx.compose.material.icons.rounded.UploadFile
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.media3.common.text.Cue
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.designsystem.components.EmptyState
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.designsystem.theme.TatakaiFonts
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.player.CueTimeline
import app.tatakai.next.player.PlaybackController
import app.tatakai.next.player.PlaybackState
import app.tatakai.next.player.PlayerKeymap
import app.tatakai.next.player.PlayerSession
import app.tatakai.next.player.SavedScreenshot
import app.tatakai.next.player.Screenshots
import app.tatakai.next.player.SleepOption
import app.tatakai.next.player.SubtitleDelay
import app.tatakai.next.player.SubtitleExtras
import app.tatakai.next.player.SubtitleParser
import app.tatakai.next.player.TimedCue
import app.tatakai.next.ui.format.formatDuration
import app.tatakai.next.ui.state.userMessage
import kotlinx.coroutines.delay
import kotlinx.coroutines.suspendCancellableCoroutine
import java.io.File
import kotlin.coroutines.resume
import app.tatakai.next.ui.R as UiR

// parity wave A (docs/redesign/15 §3.2 / §5 p, worker p): the player's new sheets, panels and helpers. Pure UI over PlayerViewModel.

// ---- keyboard shortcuts (player.shortcuts.*) ------------------------------------------------------------------------------------------

/** `KeyEvent` keycode -> [PlayerKeymap] key name (`SPACE`, `DPAD_LEFT`, `F`). */
internal fun keyNameOf(keyCode: Int): String = PlayerKeymap.keyName(android.view.KeyEvent.keyCodeToString(keyCode))

/** Human label of a key name: arrows as symbols, Space localised, `SLASH` as `?`. */
@Composable
internal fun keyLabel(key: String): String = when (key) {
    "SPACE" -> stringResource(R.string.player_key_space)
    else -> PlayerKeymap.displayName(key)
}

@Composable
internal fun actionLabel(action: String): String = stringResource(
    when (action) {
        PlayerKeymap.PLAY_PAUSE -> R.string.player_key_action_play_pause
        PlayerKeymap.SEEK_BACK -> R.string.player_key_action_seek_back
        PlayerKeymap.SEEK_FORWARD -> R.string.player_key_action_seek_forward
        PlayerKeymap.FULLSCREEN -> R.string.player_key_action_fullscreen
        PlayerKeymap.MUTE -> R.string.player_key_action_mute
        PlayerKeymap.PIP -> R.string.player_key_action_pip
        PlayerKeymap.PREV_EPISODE -> R.string.player_key_action_prev_episode
        PlayerKeymap.NEXT_EPISODE -> R.string.player_key_action_next_episode
        PlayerKeymap.VOLUME_UP -> R.string.player_key_action_volume_up
        PlayerKeymap.VOLUME_DOWN -> R.string.player_key_action_volume_down
        PlayerKeymap.SCREENSHOT -> R.string.player_key_action_screenshot
        PlayerKeymap.SUBS_TOGGLE -> R.string.player_key_action_subs_toggle
        else -> R.string.player_key_action_help
    },
)

/** The `?` panel: every action with its current key (player.shortcuts.overlay). */
@Composable
internal fun ShortcutsPanel(keymap: Map<String, String>, onDismiss: () -> Unit, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val resolved = PlayerKeymap.resolve(keymap)
    Surface(modifier.widthIn(max = 460.dp).padding(24.dp), shape = RoundedCornerShape(24.dp), color = scheme.surfaceContainerHigh) {
        Column(Modifier.padding(20.dp).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(stringResource(R.string.player_shortcuts), style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f))
                IconButton(onClick = onDismiss) { Icon(Icons.Rounded.Close, stringResource(R.string.player_shortcuts_close)) }
            }
            PlayerKeymap.ACTIONS.forEach { action ->
                Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                    Text(actionLabel(action), style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                    Surface(shape = RoundedCornerShape(8.dp), color = scheme.secondaryContainer, contentColor = scheme.onSecondaryContainer) {
                        Text(keyLabel(resolved.getValue(action)), Modifier.padding(horizontal = 10.dp, vertical = 4.dp), style = MaterialTheme.typography.labelLarge)
                    }
                }
            }
        }
    }
}

/** M / ↑ / ↓: the media stream volume (engine independent). */
internal fun adjustVolume(context: Context, direction: Int) {
    val audio = context.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
    audio.adjustStreamVolume(AudioManager.STREAM_MUSIC, direction, AudioManager.FLAG_SHOW_UI)
}

// ---- screenshots (player.screenshot, player.android.screenshot) -----------------------------------------------------------------------

/**
 * Saves the current frame to Pictures/Tatakai: mpv writes its own PNG (`screenshot-to-file`); Media3 (or an mpv failure) copies the video
 * `SurfaceView` with `PixelCopy`. Returns the saved file or null.
 */
internal suspend fun captureScreenshot(activity: Activity?, session: PlayerSession, title: String, episode: Int?): SavedScreenshot? {
    val context = activity ?: return null
    val name = Screenshots.fileName(title, episode)
    if (session !is PlaybackController) {
        val tmp = File(context.cacheDir, "screenshot.png")
        val ok = runCatching { session.screenshotToFile(tmp.absolutePath) }.getOrDefault(false)
        if (ok) return Screenshots.saveFile(context, tmp, name).also { tmp.delete() }
    }
    val surface = findSurfaceView(activity.window.decorView) ?: return null
    val w = surface.width
    val h = surface.height
    if (w <= 0 || h <= 0) return null
    val bitmap = Bitmap.createBitmap(w, h, Bitmap.Config.ARGB_8888)
    val copied = suspendCancellableCoroutine { cont ->
        try {
            PixelCopy.request(surface, bitmap, { result -> if (cont.isActive) cont.resume(result == PixelCopy.SUCCESS) }, Handler(Looper.getMainLooper()))
        } catch (e: IllegalArgumentException) {
            if (cont.isActive) cont.resume(false)
        }
    }
    if (!copied) return null
    return Screenshots.saveBitmap(context, bitmap, name).also { bitmap.recycle() }
}

private fun findSurfaceView(view: View): SurfaceView? {
    if (view is SurfaceView && view.isShown && view.width > 0) return view
    if (view is ViewGroup) {
        for (i in 0 until view.childCount) findSurfaceView(view.getChildAt(i))?.let { return it }
    }
    return null
}

// ---- subtitles drawn by the screen ------------------------------------------------------------------------------------------------------

/** Position of the session sampled every 100 ms (subtitle timing needs more than the 500 ms state tick). */
@Composable
internal fun rememberFinePosition(session: PlayerSession, active: Boolean): Long {
    var pos by remember(session) { mutableLongStateOf(session.currentPositionMs()) }
    LaunchedEffect(session, active) {
        while (active) {
            pos = session.currentPositionMs()
            delay(SUBTITLE_TICK_MS)
        }
    }
    return pos
}

/**
 * Every subtitle the screen draws: Media3's primary (engine cues, or parsed external / loaded cues with the exact delay), the secondary
 * track (Media3) at the top, and the translated line when translation is on (Media3 and mpv; mpv then hides its own text).
 */
@Composable
internal fun ScreenSubtitleLayer(
    vm: PlayerViewModel,
    session: PlayerSession,
    ps: PlaybackState,
    style: SubtitleStyle,
    extras: SubtitleExtras,
    fontFile: File?,
    bottomPadding: androidx.compose.ui.unit.Dp,
    modifier: Modifier = Modifier,
) {
    val subs by vm.screenSubs.collectAsState()
    val state by vm.state.collectAsState()
    val translating = vm.translationActive()
    val media3 = session is PlaybackController
    val needsTick = media3 || translating
    val pos = rememberFinePosition(session, needsTick)
    val delayMs = ps.subtitleDelayMs

    // Media3 engine cues, recorded for the positive delay of in-stream tracks.
    val timeline = remember(session, ps.loadId) { CueTimeline() }
    val engineLines: List<String> = if (session is PlaybackController) {
        val cues by session.cues.collectAsState()
        val texts = cueTexts(cues)
        LaunchedEffect(texts) { timeline.record(session.currentPositionMs(), texts) }
        if (delayMs > 0) timeline.linesAt(SubtitleDelay.lookupPositionMs(pos, delayMs)) else texts
    } else {
        emptyList()
    }
    val primary: List<String> = when {
        subs.primary != null && media3 -> SubtitleParser.activeAt(subs.primary!!, SubtitleDelay.lookupPositionMs(pos, delayMs))
        media3 -> if (ps.subtitlesEnabled || state.customSubtitle != null) engineLines else emptyList()
        translating -> listOfNotNull(ps.subText)
        else -> emptyList()
    }
    LaunchedEffect(translating, media3) { if (!media3) vm.onTranslationDrawing(translating) }
    LaunchedEffect(primary, translating) { if (translating) vm.translateLines(primary) }
    val shown = if (translating) primary.map { subs.translated[it] ?: it } else primary
    val secondary = subs.secondary?.let { SubtitleParser.activeAt(it, SubtitleDelay.lookupPositionMs(pos, delayMs)) }.orEmpty()

    BoxWithConstraints(modifier.fillMaxSize()) {
        if (secondary.isNotEmpty()) {
            SubtitleLines(secondary, style, extras, fontFile, Modifier.align(Alignment.TopCenter).statusBarsPadding().padding(top = 16.dp, start = 24.dp, end = 24.dp))
        }
        if (shown.isNotEmpty()) {
            val lift = maxHeight * ((100 - extras.positionPct).coerceIn(0, 100) / 100f) * 0.8f
            SubtitleLines(shown, style, extras, fontFile, Modifier.align(Alignment.BottomCenter).padding(bottom = bottomPadding + lift, start = 24.dp, end = 24.dp))
        }
    }
}

private fun cueTexts(cues: List<Cue>): List<String> = cues.mapNotNull { it.text?.toString()?.takeIf { t -> t.isNotBlank() } }

/** One block of subtitle lines with [SubtitleStyle] + [SubtitleExtras] (colour, outline, imported font). */
@Composable
internal fun SubtitleLines(lines: List<String>, style: SubtitleStyle, extras: SubtitleExtras, fontFile: File?, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    val family = remember(fontFile, style.font) {
        fontFile?.let { f -> runCatching { FontFamily(Font(f)) }.getOrNull() } ?: when (style.font) {
            "system" -> FontFamily.SansSerif
            "space_grotesk" -> TatakaiFonts.families.display
            else -> TatakaiFonts.families.text
        }
    }
    val scale = style.scale.coerceIn(0.5f, 2f)
    val shadow = if (style.outline && extras.outlineSize > 0f) Shadow(color = Color(extras.outlineColor), blurRadius = 2f + extras.outlineSize * 2f) else null
    val textStyle = TextStyle(
        fontFamily = family,
        fontWeight = if (style.bold) FontWeight.Bold else FontWeight.Medium,
        fontSize = (18 * scale).sp,
        lineHeight = (24 * scale).sp,
        textAlign = TextAlign.Center,
        color = Color(extras.textColor),
        shadow = shadow,
    )
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp)) {
        lines.forEach { line ->
            Text(
                line,
                style = textStyle,
                modifier = Modifier
                    .background(scheme.scrim.copy(alpha = style.backgroundOpacity.coerceIn(0f, 1f)), RoundedCornerShape(6.dp))
                    .padding(horizontal = 8.dp, vertical = 2.dp),
            )
        }
    }
}

/** Pre-translation progress over the paused video, with "Play now". */
@Composable
internal fun PretranslateOverlay(progress: Pair<Int, Int>, onSkip: () -> Unit, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Surface(modifier.widthIn(max = 420.dp).padding(24.dp), shape = RoundedCornerShape(24.dp), color = scheme.surfaceContainerHigh) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Icon(Icons.Rounded.Translate, contentDescription = null, tint = scheme.primary)
            Text(stringResource(R.string.player_pretranslating, progress.first, progress.second), textAlign = TextAlign.Center)
            LinearProgressIndicator(
                progress = { if (progress.second > 0) progress.first.toFloat() / progress.second else 0f },
                modifier = Modifier.fillMaxWidth(),
            )
            TextButton(onClick = onSkip) { Text(stringResource(R.string.player_pretranslate_skip)) }
        }
    }
}

// ---- sheets -----------------------------------------------------------------------------------------------------------------------------

@Composable
private fun SheetTitle(text: String) {
    Text(text, style = MaterialTheme.typography.titleSmall, color = MaterialTheme.colorScheme.primary, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp))
}

@Composable
private fun CheckRow(text: String, selected: Boolean, onClick: () -> Unit) {
    ListItem(
        headlineContent = { Text(text, maxLines = 1, overflow = TextOverflow.Ellipsis) },
        trailingContent = { if (selected) Icon(Icons.Rounded.Check, contentDescription = null) },
        modifier = Modifier.clickable(onClick = onClick),
    )
}

/**
 * Subtitles sheet: tracks (+ the loaded file), "Load file…", delay ±0.1 s, secondary track, translation toggle and "Pre-translate".
 */
@Composable
internal fun SubtitlesSheetContent(
    vm: PlayerViewModel,
    session: PlayerSession,
    ps: PlaybackState,
    state: PlayerUiState,
    onLoadFile: () -> Unit,
    onDone: () -> Unit,
) {
    Column(Modifier.verticalScroll(rememberScrollState())) {
        if (!ps.fullAss && ps.subtitles.any { it.ass }) {
            ListItem(headlineContent = { Text(stringResource(R.string.player_ass_simplified), style = MaterialTheme.typography.bodySmall) })
        }
        val custom = state.customSubtitle
        CheckRow(stringResource(R.string.player_subtitles_off), !ps.subtitlesEnabled && custom == null) { vm.selectSubtitle(null); onDone() }
        ps.subtitles.forEach { o -> CheckRow(o.label, o.selected && custom == null) { vm.selectSubtitle(o.groupIndex); onDone() } }
        if (custom != null) CheckRow(stringResource(R.string.player_sub_custom, custom.name), true) {}
        ListItem(
            headlineContent = { Text(stringResource(R.string.player_sub_load)) },
            leadingContent = { Icon(Icons.Rounded.UploadFile, contentDescription = null) },
            modifier = Modifier.clickable { onLoadFile() },
        )
        HorizontalDivider()
        SheetTitle(stringResource(R.string.player_sub_delay))
        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            FilledTonalIconButton(onClick = { vm.stepSubtitleDelay(-1) }) { Icon(Icons.Rounded.Remove, stringResource(R.string.player_sub_delay_earlier)) }
            Text(stringResource(R.string.player_sub_delay_value, SubtitleDelay.format(ps.subtitleDelayMs)), style = MaterialTheme.typography.titleMedium)
            FilledTonalIconButton(onClick = { vm.stepSubtitleDelay(1) }) { Icon(Icons.Rounded.Add, stringResource(R.string.player_sub_delay_later)) }
            IconButton(onClick = { vm.stepSubtitleDelay(0) }, enabled = ps.subtitleDelayMs != 0L) {
                Icon(Icons.Rounded.Restore, stringResource(R.string.player_sub_delay_reset))
            }
        }
        if (session is PlaybackController && ps.subtitleDelayMs < 0) {
            Text(
                stringResource(R.string.player_sub_delay_media3_hint),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            )
        }
        HorizontalDivider(Modifier.padding(top = 8.dp))
        SheetTitle(stringResource(R.string.player_sub_secondary))
        if (session.supportsSecondaryTracks) {
            CheckRow(stringResource(R.string.player_sub_secondary_off), ps.secondarySubtitleId == null) { vm.selectSecondary(null, null) }
            ps.subtitles.forEach { o -> CheckRow(o.label, ps.secondarySubtitleId == o.groupIndex) { vm.selectSecondary(o.groupIndex, null) } }
        } else if (state.externalSubtitles.isEmpty()) {
            Text(
                stringResource(R.string.player_sub_secondary_none),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp),
            )
        } else {
            CheckRow(stringResource(R.string.player_sub_secondary_off), state.secondaryExternal == null) { vm.selectSecondary(null, null) }
            state.externalSubtitles.forEachIndexed { i, t ->
                CheckRow(t.label.ifBlank { t.lang }, state.secondaryExternal == i) { vm.selectSecondary(null, i) }
            }
        }
        if (state.translation.available) {
            HorizontalDivider(Modifier.padding(top = 8.dp))
            ListItem(
                headlineContent = { Text(stringResource(R.string.player_sub_translate)) },
                supportingContent = {
                    val t = state.settings.subtitleTranslation
                    Text(state.translation.error ?: stringResource(R.string.player_sub_translate_hint, providerLabel(t.provider), t.targetLang.uppercase()))
                },
                leadingContent = { Icon(Icons.Rounded.Translate, contentDescription = null) },
                trailingContent = { Switch(checked = state.translation.enabled, onCheckedChange = vm::setTranslationEnabled) },
            )
            if (state.translation.enabled) {
                TextButton(onClick = { vm.pretranslate(); onDone() }, modifier = Modifier.padding(horizontal = 8.dp)) {
                    Text(stringResource(R.string.player_sub_pretranslate))
                }
            }
        }
    }
}

/** "More" sheet: sleep timer, loop, stable volume. */
@Composable
internal fun MoreSheetContent(vm: PlayerViewModel, state: PlayerUiState) {
    Column(Modifier.verticalScroll(rememberScrollState())) {
        SheetTitle(stringResource(R.string.player_sleep))
        val sleep = state.sleep
        SleepOption.entries.forEach { o ->
            val label = when (o) {
                SleepOption.OFF -> stringResource(R.string.player_sleep_off)
                SleepOption.END_OF_EPISODE -> stringResource(R.string.player_sleep_end)
                else -> stringResource(R.string.player_sleep_minutes, o.minutes ?: 0)
            }
            val selected = if (o == SleepOption.OFF) sleep == null else sleep?.option == o
            ListItem(
                headlineContent = { Text(label) },
                supportingContent = if (selected && sleep?.remainingMs != null) {
                    { Text(stringResource(R.string.player_sleep_remaining, formatDuration((sleep.remainingMs ?: 0) / 1000))) }
                } else {
                    null
                },
                leadingContent = { Icon(Icons.Rounded.Bedtime, contentDescription = null) },
                trailingContent = { if (selected) Icon(Icons.Rounded.Check, contentDescription = null) },
                modifier = Modifier.clickable { vm.setSleepTimer(o) },
            )
        }
        HorizontalDivider()
        ListItem(
            headlineContent = { Text(stringResource(R.string.player_loop)) },
            supportingContent = { Text(stringResource(R.string.player_loop_hint)) },
            trailingContent = { Switch(checked = state.loop, onCheckedChange = vm::setLoop) },
        )
        ListItem(
            headlineContent = { Text(stringResource(R.string.player_stable_volume)) },
            supportingContent = { Text(stringResource(R.string.player_stable_volume_hint)) },
            trailingContent = { Switch(checked = state.settings.stableVolume, onCheckedChange = vm::setStableVolume) },
        )
    }
}

/** Episode list over the player (player.jump-to-episode): loading / error / empty states, the current one marked. */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun EpisodesSheetContent(vm: PlayerViewModel, state: PlayerUiState, current: Int, onPick: (Int) -> Unit) {
    LaunchedEffect(Unit) { vm.loadEpisodes() }
    val eps = state.episodes
    Column(Modifier.heightIn(min = 200.dp, max = 560.dp)) {
        SheetTitle(stringResource(R.string.player_episodes))
        when {
            eps.loading -> {
                val show = rememberAfterDelay(150)
                Box(Modifier.fillMaxWidth().padding(32.dp), contentAlignment = Alignment.Center) { if (show) LoadingIndicator(Modifier.size(48.dp)) }
            }
            eps.error != null -> ErrorState(
                message = eps.error.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = { vm.loadEpisodes(force = true) },
                title = stringResource(R.string.player_episodes_error),
                compact = true,
            )
            eps.loaded && eps.items.isEmpty() -> EmptyState(icon = Icons.Rounded.PlayArrow, title = stringResource(R.string.player_episodes_empty))
            else -> LazyColumn {
                items(eps.items, key = { it.number }) { e ->
                    val title = e.title?.takeIf { it.isNotBlank() }
                    ListItem(
                        headlineContent = {
                            Text(
                                if (title != null) stringResource(R.string.player_episode_row_title, e.number, title) else stringResource(R.string.player_episode_row, e.number),
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        },
                        supportingContent = if (e.number == current) ({ Text(stringResource(R.string.player_episode_current)) }) else null,
                        trailingContent = { if (e.number == current) Icon(Icons.Rounded.PlayArrow, contentDescription = null) },
                        modifier = Modifier.clickable(enabled = e.number != current) { onPick(e.number) },
                    )
                }
            }
        }
    }
}

/** Locked screen (player.android.screen-lock): only the lock button, which hides after a moment and comes back on tap. */
@Composable
internal fun LockOverlay(visible: Boolean, onShow: () -> Unit, onUnlock: () -> Unit) {
    Box(
        Modifier.fillMaxSize().clickable(interactionSource = null, indication = null) { onShow() },
        contentAlignment = Alignment.CenterStart,
    ) {
        if (visible) {
            Column(Modifier.padding(start = 32.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
                FilledTonalButton(onClick = onUnlock) {
                    Icon(Icons.Rounded.Lock, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.player_unlock), Modifier.padding(start = 8.dp))
                }
                Text(stringResource(R.string.player_locked_hint), color = MaterialTheme.colorScheme.onBackground, style = MaterialTheme.typography.labelMedium)
            }
        }
    }
}

@Composable
internal fun providerLabel(p: app.tatakai.next.model.TranslationProvider): String = stringResource(
    when (p) {
        app.tatakai.next.model.TranslationProvider.NONE -> R.string.player_settings_provider_none
        app.tatakai.next.model.TranslationProvider.GOOGLE -> R.string.player_settings_provider_google
        app.tatakai.next.model.TranslationProvider.DEEPL -> R.string.player_settings_provider_deepl
        app.tatakai.next.model.TranslationProvider.DEEPLX -> R.string.player_settings_provider_deeplx
        app.tatakai.next.model.TranslationProvider.AZURE -> R.string.player_settings_provider_azure
        app.tatakai.next.model.TranslationProvider.OPENAI -> R.string.player_settings_provider_openai
    },
)

internal const val SUBTITLE_TICK_MS = 100L
