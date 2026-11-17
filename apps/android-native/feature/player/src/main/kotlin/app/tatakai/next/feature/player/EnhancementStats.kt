// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import app.tatakai.next.feature.link.linkFrameGenText
import app.tatakai.next.feature.link.linkUpscaleText
import app.tatakai.next.framegen.FgStatsStatus
import app.tatakai.next.framegen.FgStatsView
import app.tatakai.next.framegen.FrameGenText
import app.tatakai.next.framegen.FrameGenUiState
import app.tatakai.next.model.ProcessingLocation
import app.tatakai.next.player.QualityLevel
import app.tatakai.next.player.QualityUiState
import app.tatakai.next.player.decoderInUseLabel
import java.util.Locale
import app.tatakai.next.framegen.R as FgR

// 0.4.0 (worker D): the "Estadísticas" overlay, adapted from Enigmata's QualityStatsOverlay.kt + FrameGenStatsOverlay.kt
// (AGPL-3.0-or-later, same author). Plain Column + background, no Surface and no pointer input: touches go through to the
// player's gestures underneath (a Material Surface would swallow them).

/** Requested vs effective, active shaders, decoder, drops, fps / display Hz, renderer, the phone's FG and what the PC does. */
@Composable
internal fun EnhancementStatsOverlay(
    quality: QualityUiState,
    processing: ProcessingUiState,
    fgState: FrameGenUiState?,
    modifier: Modifier = Modifier,
) {
    val text = Color.White
    Column(
        modifier
            .widthIn(max = 380.dp)
            .background(Color.Black.copy(alpha = 0.62f), RoundedCornerShape(12.dp))
            .padding(horizontal = 12.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        @Composable
        fun line(s: String, color: Color = text) =
            Text(s, color = color, style = MaterialTheme.typography.labelSmall, fontFamily = FontFamily.Monospace)

        Text(stringResource(R.string.player_stats_title), color = text, style = MaterialTheme.typography.labelLarge)
        if (quality.available) {
            val req = QualityLevel.of(quality.requested.preset, quality.requested.tier)
            line(stringResource(R.string.player_stats_requested, levelText(req)))
            line(stringResource(R.string.player_stats_effective, levelText(quality.effective)))
            quality.reasons.forEach { line("· " + stringResource(it.labelRes), Color(0xFFFFD180)) }
            val shaders = quality.activeShaders.joinToString(" → ") { it.substringAfterLast('/').removeSuffix(".glsl") }
                .ifEmpty { stringResource(R.string.player_stats_none) } + if (quality.fp16) " · FP16" else ""
            line(stringResource(R.string.player_stats_shaders, shaders))
            line(
                stringResource(
                    R.string.player_stats_decoder,
                    decoderInUseLabel(quality.decoderInUse) ?: "—",
                    stringResource(decoderRes(quality.requested.decoder)),
                ),
            )
            line(stringResource(R.string.player_stats_drops, quality.droppedFrames))
            line(stringResource(R.string.player_stats_fps, quality.videoFps?.let { fps2(it) } ?: "—", quality.displayHz?.let { fps2(it.toDouble()) } ?: "—"))
            if (quality.videoWidth > 0) {
                line(stringResource(R.string.player_stats_size, quality.videoWidth, quality.videoHeight, quality.viewportWidth, quality.viewportHeight))
            }
            quality.renderer?.let { line(stringResource(R.string.player_stats_renderer, it)) }
        }
        if (fgState != null && quality.available) {
            val v = FgStatsView.from(fgState, fgState.liveFps)
            when {
                v.status == FgStatsStatus.ACTIVE || v.status == FgStatsStatus.PAUSED -> line(
                    stringResource(R.string.player_stats_fg, fpsText(v.inFps), fpsText(v.outFps), v.multiplier, fps2(v.latencyMs)),
                    if (v.overloaded) Color(0xFFFFD180) else text,
                )
                v.reason != null -> line(stringResource(R.string.player_stats_fg_status, stringResource(FrameGenText.reason(v.reason!!, v.noInputTimeout))))
                else -> line(stringResource(R.string.player_stats_fg_status, stringResource(FgR.string.fg_state_off)))
            }
        }
        val pc = processing.pc
        if (processing.location != ProcessingLocation.PHONE && pc != null) {
            line(stringResource(R.string.player_stats_pc, listOfNotNull(linkUpscaleText(pc), linkFrameGenText(pc)).joinToString(" + ")))
        }
    }
}

private fun fps2(v: Double): String = String.format(Locale.ROOT, "%.2f", v).trimEnd('0').trimEnd('.')
