// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.theme

import androidx.compose.runtime.Composable
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.staticCompositionLocalOf

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Names and signatures FIXED.

/**
 * true = reduce motion: no hero autoplay, no wavy phase animation, no bounces, instant transitions. Provided once by the app root
 * (`CompositionLocalProvider(LocalReduceMotion provides rememberReduceMotion(settings.reduceMotion))`); everybody else only reads it.
 */
val LocalReduceMotion: ProvidableCompositionLocal<Boolean> = staticCompositionLocalOf { false }

/** [settingEnabled] (Settings › Apariencia) OR the system "remove animations" (ANIMATOR_DURATION_SCALE == 0). Body: worker b. */
@Composable
fun rememberReduceMotion(settingEnabled: Boolean): Boolean = settingEnabled
