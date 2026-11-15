// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.look

import androidx.compose.animation.core.AnimationSpec
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.snap
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.interaction.InteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.getValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.theme.LocalReduceMotion
import app.tatakai.next.model.Density
import app.tatakai.next.model.LookPrefs
import app.tatakai.next.model.Motion

// 2026-11 UI refresh: the user's Diseño y estilo choices (AppSettings.look), provided once by the app shell (TatakaiApp) like
// LocalTitleLanguage, so shared components (MediaCard, episode rows, the hero) follow them without extra parameters.

val LocalLook: ProvidableCompositionLocal<LookPrefs> = staticCompositionLocalOf { LookPrefs() }

/**
 * Effective motion level: [Motion.OFF] when the `motion` pref is off, Settings › Reducir movimiento is on or the system removed
 * animations (LocalReduceMotion already folds the last two).
 */
@Composable
fun lookMotion(): Motion = if (LocalReduceMotion.current) Motion.OFF else LocalLook.current.motion

/** True when spatial motion (scale, slides, bounces) is allowed: only the expressive level. */
@Composable
fun spatialMotion(): Boolean = lookMotion() == Motion.EXPRESSIVE

/** Opacity / color animation spec of the current level (instant when off). */
@Composable
fun <T> lookFadeSpec(durationMs: Int = 180): AnimationSpec<T> = if (lookMotion() == Motion.OFF) snap() else tween(durationMs)

/** Horizontal gap between cards in rows (compact density packs them tighter). */
@Composable
fun rowSpacing(): Dp = if (LocalLook.current.density == Density.COMPACT) 8.dp else 12.dp

/** Poster card width in rows. */
@Composable
fun cardWidth(): Dp = if (LocalLook.current.density == Density.COMPACT) 104.dp else 120.dp

/**
 * Press feedback: scales the node to 0.96 with a spring while [interactionSource] is pressed (expressive motion only; graphicsLayer
 * scale, no relayout). Subtle / off motion leave it still.
 */
@Composable
fun Modifier.pressScale(interactionSource: InteractionSource, pressedScale: Float = 0.96f): Modifier {
    val pressed by interactionSource.collectIsPressedAsState()
    val enabled = spatialMotion()
    val scale = animateFloatAsState(
        targetValue = if (pressed && enabled) pressedScale else 1f,
        animationSpec = if (enabled) spring(dampingRatio = Spring.DampingRatioMediumBouncy, stiffness = Spring.StiffnessMediumLow) else snap(),
        label = "pressScale",
    )
    // Read in the layer block: the animation only re-draws, it never recomposes the card.
    return this.graphicsLayer {
        scaleX = scale.value
        scaleY = scale.value
    }
}
