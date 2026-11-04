// SPDX-License-Identifier: AGPL-3.0-or-later
@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.designsystem.components

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.material3.CircularWavyProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.LinearWavyProgressIndicator
import androidx.compose.material3.WavyProgressIndicatorDefaults
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.theme.LocalReduceMotion

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Names and signatures FIXED.

/**
 * M3E wavy progress (`LinearWavyProgressIndicator` / `CircularWavyProgressIndicator`, confirmed in material3 1.5.0-alpha15).
 * [progress] 0..1, null = indeterminate. The wave phase does not animate when `LocalReduceMotion` is true. [thin] = 4 dp track.
 *
 * Reduce motion: `waveSpeed = 0.dp` (no phase animation) and a flat line for determinate progress (amplitude 0). Not thin = the
 * "thick" variant of the Material guidelines (8 dp stroke on a 14 dp container) for linear, 6 dp stroke / 56 dp for circular.
 */
@Composable
fun WavyProgress(
    progress: Float?,
    modifier: Modifier = Modifier,
    circular: Boolean = false,
    thin: Boolean = false,
) {
    val reduceMotion = LocalReduceMotion.current
    val density = LocalDensity.current
    val strokeWidth = with(density) { (if (thin) 4.dp else if (circular) 6.dp else 8.dp).toPx() }
    val stroke = Stroke(width = strokeWidth, cap = StrokeCap.Round)
    val flat: (Float) -> Float = { 0f }
    if (circular) {
        val size = if (thin) 40.dp else 56.dp
        if (progress != null) {
            val clamped = progress.coerceIn(0f, 1f)
            CircularWavyProgressIndicator(
                progress = { clamped },
                modifier = modifier.size(size),
                stroke = stroke,
                trackStroke = stroke,
                amplitude = if (reduceMotion) flat else WavyProgressIndicatorDefaults.indicatorAmplitude,
                waveSpeed = if (reduceMotion) 0.dp else WavyProgressIndicatorDefaults.CircularWavelength,
            )
        } else {
            CircularWavyProgressIndicator(
                modifier = modifier.size(size),
                stroke = stroke,
                trackStroke = stroke,
                amplitude = if (reduceMotion) 0f else 1f,
                waveSpeed = if (reduceMotion) 0.dp else WavyProgressIndicatorDefaults.CircularWavelength,
            )
        }
    } else {
        val height = if (thin) WavyProgressIndicatorDefaults.LinearContainerHeight else 14.dp
        if (progress != null) {
            val clamped = progress.coerceIn(0f, 1f)
            LinearWavyProgressIndicator(
                progress = { clamped },
                modifier = modifier.fillMaxWidth().height(height),
                stroke = stroke,
                trackStroke = stroke,
                amplitude = if (reduceMotion) flat else WavyProgressIndicatorDefaults.indicatorAmplitude,
                waveSpeed = if (reduceMotion) 0.dp else WavyProgressIndicatorDefaults.LinearDeterminateWavelength,
            )
        } else {
            LinearWavyProgressIndicator(
                modifier = modifier.fillMaxWidth().height(height),
                stroke = stroke,
                trackStroke = stroke,
                amplitude = if (reduceMotion) 0f else 1f,
                waveSpeed = if (reduceMotion) 0.dp else WavyProgressIndicatorDefaults.LinearIndeterminateWavelength,
            )
        }
    }
}
