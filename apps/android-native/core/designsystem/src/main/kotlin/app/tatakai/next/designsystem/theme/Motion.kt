// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.theme

import android.content.ContentResolver
import android.database.ContentObserver
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.ProvidableCompositionLocal
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.platform.LocalContext

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Names and signatures FIXED.

/**
 * true = reduce motion: no hero autoplay, no wavy phase animation, no bounces, instant transitions. Provided once by the app root
 * (`CompositionLocalProvider(LocalReduceMotion provides rememberReduceMotion(settings.reduceMotion))`); everybody else only reads it.
 */
val LocalReduceMotion: ProvidableCompositionLocal<Boolean> = staticCompositionLocalOf { false }

/** [settingEnabled] (Settings › Apariencia) OR the system "remove animations" (ANIMATOR_DURATION_SCALE == 0). */
@Composable
fun rememberReduceMotion(settingEnabled: Boolean): Boolean {
    val resolver = LocalContext.current.applicationContext.contentResolver
    var systemOff by remember(resolver) { mutableStateOf(animationsDisabled(resolver)) }
    DisposableEffect(resolver) {
        val observer = object : ContentObserver(Handler(Looper.getMainLooper())) {
            override fun onChange(selfChange: Boolean) {
                systemOff = animationsDisabled(resolver)
            }
        }
        val uri = Settings.Global.getUriFor(Settings.Global.ANIMATOR_DURATION_SCALE)
        runCatching { resolver.registerContentObserver(uri, false, observer) }
        systemOff = animationsDisabled(resolver)
        onDispose { runCatching { resolver.unregisterContentObserver(observer) } }
    }
    return settingEnabled || systemOff
}

private fun animationsDisabled(resolver: ContentResolver): Boolean =
    runCatching { Settings.Global.getFloat(resolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f }.getOrDefault(false)
