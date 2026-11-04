// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(UnstableApi::class)

package app.tatakai.next.feature.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.CellTower
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.Close
import androidx.compose.material.icons.rounded.ErrorOutline
import androidx.compose.material.icons.rounded.FastForward
import androidx.compose.material.icons.rounded.SkipNext
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LoadingIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.media3.common.text.Cue
import androidx.media3.common.util.UnstableApi
import app.tatakai.next.designsystem.theme.TatakaiFonts
import app.tatakai.next.model.SkipKind
import app.tatakai.next.model.SubtitleStyle
import app.tatakai.next.player.FailKind
import app.tatakai.next.ui.format.formatBytes
import app.tatakai.next.ui.format.formatRate
import app.tatakai.next.ui.state.userMessage
import kotlinx.coroutines.delay
import kotlin.math.roundToInt
import app.tatakai.next.ui.R as UiR

/** "4.2 MB/s · 12 peers · 37 % of the file". */
@Composable
internal fun torrentLine(t: TorrentUi): String =
    stringResource(R.string.player_torrent_status, formatRate(t.downBps), t.peers, (t.progress * 100).roundToInt().coerceIn(0, 100))

/** Overlay text of the torrent start: "Waiting for torrent metadata…" then speed / peers / %. */
@Composable
internal fun TorrentStatusText(t: TorrentUi, modifier: Modifier = Modifier) {
    val on = MaterialTheme.colorScheme.onBackground
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(4.dp)) {
        if (t.waitingMetadata) {
            Text(stringResource(R.string.player_torrent_metadata), color = on, textAlign = TextAlign.Center)
        }
        Text(torrentLine(t), color = on.copy(alpha = 0.85f), style = MaterialTheme.typography.labelLarge, fontFamily = FontFamily.Monospace)
        t.fileName?.let {
            Text(stringResource(R.string.player_torrent_file, it), color = on.copy(alpha = 0.7f), style = MaterialTheme.typography.labelMedium, maxLines = 1)
        }
    }
}

/** Buffering / resolving overlay on top of a running engine (failover, torrent start, stalls). */
@OptIn(ExperimentalMaterial3ExpressiveApi::class)
@Composable
internal fun BufferingOverlay(state: PlayerUiState, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Surface(modifier, shape = RoundedCornerShape(24.dp), color = scheme.surfaceContainerHigh.copy(alpha = 0.85f), contentColor = scheme.onSurface) {
        Column(Modifier.padding(20.dp), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(12.dp)) {
            LoadingIndicator(Modifier.size(48.dp))
            val t = state.torrent
            if (t != null) {
                TorrentStatusText(t)
            } else if (state.phase == PlayerPhase.RESOLVING) {
                val name = state.current?.candidate?.let { "${it.extName} · ${it.label}" }
                Text(if (name != null) stringResource(R.string.player_resolving_from, name) else stringResource(R.string.player_resolving))
            }
        }
    }
}

/** Short confirmation over the video ("Intro skipped", "Trying another source"); disappears by itself. */
@Composable
internal fun FlashToast(notice: PlayerNotice, onDone: () -> Unit, modifier: Modifier = Modifier) {
    LaunchedEffect(notice) {
        delay(if (notice is PlayerNotice.AssSimplified) 5_000 else FLASH_MS)
        onDone()
    }
    val text = when (notice) {
        PlayerNotice.TryingNext -> stringResource(R.string.player_fallback_next)
        is PlayerNotice.NoFileNext -> stringResource(R.string.player_torrent_no_file_next, notice.episode)
        PlayerNotice.IntroSkipped -> stringResource(R.string.player_intro_skipped)
        PlayerNotice.AssSimplified -> stringResource(R.string.player_ass_simplified)
    }
    val scheme = MaterialTheme.colorScheme
    Surface(modifier.widthIn(max = 560.dp).padding(horizontal = 16.dp), shape = RoundedCornerShape(50), color = scheme.inverseSurface, contentColor = scheme.inverseOnSurface) {
        Row(Modifier.padding(horizontal = 16.dp, vertical = 10.dp), Arrangement.spacedBy(8.dp), Alignment.CenterVertically) {
            Icon(Icons.Rounded.Check, contentDescription = null, modifier = Modifier.size(18.dp))
            Text(text, style = MaterialTheme.typography.labelLarge)
        }
    }
}

/** "Skip intro / recap / ending" while the position is inside a mark. */
@Composable
internal fun SkipButton(kind: SkipKind, onClick: () -> Unit, modifier: Modifier = Modifier) {
    val label = when (kind) {
        SkipKind.INTRO -> R.string.player_jump_intro
        SkipKind.OUTRO -> R.string.player_jump_outro
        SkipKind.RECAP -> R.string.player_jump_recap
    }
    FilledTonalButton(onClick = onClick, modifier = modifier) {
        Icon(Icons.Rounded.FastForward, contentDescription = null, modifier = Modifier.size(18.dp))
        Text(stringResource(label), Modifier.padding(start = 8.dp))
    }
}

/** Next-episode card with the 10 s countdown ring (prototype `player.js` `ended()`). */
@Composable
internal fun NextEpisodeCard(next: NextEpisodeUi, onPlayNow: () -> Unit, onCancel: () -> Unit, modifier: Modifier = Modifier) {
    val scheme = MaterialTheme.colorScheme
    Surface(modifier, shape = RoundedCornerShape(20.dp), color = scheme.surfaceContainerHigh, contentColor = scheme.onSurface) {
        Row(Modifier.padding(12.dp), Arrangement.spacedBy(12.dp), Alignment.CenterVertically) {
            Box(contentAlignment = Alignment.Center) {
                CircularProgressIndicator(progress = { next.secondsLeft / 10f }, modifier = Modifier.size(44.dp))
                Text("${next.secondsLeft}", style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.Bold)
            }
            Column(Modifier.widthIn(max = 220.dp)) {
                Text(stringResource(R.string.player_next), style = MaterialTheme.typography.titleSmall)
                Text(stringResource(R.string.player_next_episode_n, next.episode), style = MaterialTheme.typography.bodySmall, color = scheme.onSurfaceVariant)
                if (next.sameTorrent) {
                    Text(stringResource(R.string.player_next_same_torrent), style = MaterialTheme.typography.labelSmall, color = scheme.primary)
                }
            }
            IconButton(onClick = onPlayNow) { Icon(Icons.Rounded.SkipNext, stringResource(R.string.player_play_now)) }
            IconButton(onClick = onCancel) { Icon(Icons.Rounded.Close, stringResource(R.string.player_cancel)) }
        }
    }
}

/** Mobile data confirmation before a torrent (11 §4.1 step 2). */
@Composable
internal fun MeteredDialog(prompt: MeteredPrompt, onWatch: () -> Unit, onWeb: () -> Unit, onSettings: () -> Unit) {
    AlertDialog(
        onDismissRequest = onWeb,
        icon = { Icon(Icons.Rounded.CellTower, contentDescription = null) },
        title = { Text(stringResource(R.string.player_metered_title)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Text(
                    prompt.sizeBytes?.let { stringResource(R.string.player_metered_body, formatBytes(it)) }
                        ?: stringResource(R.string.player_metered_body_unknown),
                )
                TextButton(onClick = onSettings) { Text(stringResource(R.string.player_metered_settings)) }
            }
        },
        confirmButton = { Button(onClick = onWatch) { Text(stringResource(R.string.player_metered_watch)) } },
        dismissButton = {
            TextButton(onClick = onWeb) {
                Text(stringResource(if (prompt.hasWeb) R.string.player_metered_web else R.string.player_cancel))
            }
        },
    )
}

/** Final ErrorState of the source flow (or of a direct source) with `failed[]` and the ways out. */
@Composable
internal fun PlaybackErrorView(
    state: PlayerUiState,
    onRetry: () -> Unit,
    onUseWeb: () -> Unit,
    onPick: () -> Unit,
    onOpenTorrent: (String) -> Unit,
    onOpenExtensions: () -> Unit,
) {
    val scheme = MaterialTheme.colorScheme
    val noFile = state.noFileTorrentId
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(Icons.Rounded.ErrorOutline, contentDescription = null, tint = scheme.error, modifier = Modifier.size(56.dp))
        Text(stringResource(R.string.player_error_title), style = MaterialTheme.typography.titleLarge, color = scheme.onBackground)
        val cause = when {
            noFile != null -> stringResource(R.string.player_torrent_no_file_cause)
            state.failed.isEmpty() && state.error != null -> state.error.userMessage()
            else -> stringResource(R.string.player_error_no_more)
        }
        Text(cause, style = MaterialTheme.typography.bodyMedium, color = scheme.onSurfaceVariant, textAlign = TextAlign.Center)
        if (state.failed.isNotEmpty()) {
            Surface(shape = MaterialTheme.shapes.medium, color = scheme.surfaceContainer, modifier = Modifier.widthIn(max = 560.dp).fillMaxWidth()) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    Text(stringResource(R.string.player_failed_list), style = MaterialTheme.typography.labelLarge)
                    state.failed.forEach { f ->
                        Text(
                            stringResource(R.string.player_failed_entry, f.name, failReason(f.kind, f.detail)),
                            style = MaterialTheme.typography.bodySmall,
                            fontFamily = FontFamily.Monospace,
                            color = scheme.onSurfaceVariant,
                        )
                    }
                }
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Button(onClick = onRetry) { Text(stringResource(UiR.string.ui_retry)) }
            if (state.canPickSource) OutlinedButton(onClick = onPick) { Text(stringResource(R.string.player_pick_source)) }
        }
        if (state.lastFailedTorrent && state.hasWeb) {
            FilledTonalButton(onClick = onUseWeb) { Text(stringResource(R.string.player_use_web)) }
        }
        if (noFile != null) {
            FilledTonalButton(onClick = { onOpenTorrent(noFile) }) { Text(stringResource(R.string.player_torrent_pick_file)) }
        }
        TextButton(onClick = onOpenExtensions) { Text(stringResource(R.string.player_go_extensions)) }
    }
}

@Composable
private fun failReason(kind: FailKind, detail: String): String = when (kind) {
    FailKind.NO_PEERS -> stringResource(R.string.player_fail_no_peers)
    FailKind.NO_FILE -> stringResource(R.string.player_fail_no_file)
    FailKind.METERED_DECLINED -> stringResource(R.string.player_fail_metered)
    FailKind.RESOLVE -> stringResource(R.string.player_fail_resolve)
    FailKind.PLAYBACK, FailKind.UNKNOWN -> detail.ifBlank { stringResource(R.string.player_fail_resolve) }
}

/**
 * Media3 subtitle cues drawn by us (ContentFrame does not render text): Settings › Estilo de subtítulos = scale, background box opacity,
 * font (Outfit / Space Grotesk / system), bold and outline. ASS styling is lost under Media3 (the player shows the simplified notice).
 */
@Composable
internal fun SubtitleOverlay(cues: List<Cue>, style: SubtitleStyle, modifier: Modifier = Modifier) {
    val texts = cues.mapNotNull { it.text?.toString()?.takeIf { t -> t.isNotBlank() } }
    if (texts.isEmpty()) return
    val scheme = MaterialTheme.colorScheme
    val family = when (style.font) {
        "system" -> FontFamily.SansSerif
        "space_grotesk" -> TatakaiFonts.families.display
        else -> TatakaiFonts.families.text
    }
    val fg = scheme.onBackground
    val shadow = if (style.outline) Shadow(color = scheme.background, blurRadius = 6f) else null
    val textStyle = TextStyle(
        fontFamily = family,
        fontWeight = if (style.bold) FontWeight.Bold else FontWeight.Medium,
        fontSize = (18 * style.scale.coerceIn(0.5f, 2f)).sp,
        lineHeight = (24 * style.scale.coerceIn(0.5f, 2f)).sp,
        textAlign = TextAlign.Center,
        color = fg,
        shadow = shadow,
    )
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp)) {
        texts.forEach { line ->
            Text(
                line,
                style = textStyle,
                modifier = Modifier
                    .background(scheme.background.copy(alpha = style.backgroundOpacity.coerceIn(0f, 1f)), RoundedCornerShape(6.dp))
                    .padding(horizontal = 8.dp, vertical = 2.dp),
            )
        }
    }
}

internal const val FLASH_MS = 2_600L
