@file:OptIn(ExperimentalMaterial3ExpressiveApi::class)

package app.tatakai.next.designsystem.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.ExperimentalMaterial3ExpressiveApi
import androidx.compose.material3.MaterialExpressiveTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MotionScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import app.tatakai.next.designsystem.generated.ContrastLevel
import app.tatakai.next.designsystem.generated.TatakaiTokens
import com.materialkolor.DynamicMaterialExpressiveTheme
import com.materialkolor.PaletteStyle

/**
 * The light/dark mode the app resolved (Settings › Apariencia › Modo, or the system when it is "Sistema"), so nested
 * themes such as [TatakaiSeedTheme] follow the app instead of `isSystemInDarkTheme()`.
 */
// Outside TatakaiTheme (a bare preview) both default to light / not AMOLED.
val LocalTatakaiDarkTheme = staticCompositionLocalOf { false }

/** Whether the app is on Negro AMOLED (only meaningful while [LocalTatakaiDarkTheme] is true). */
val LocalTatakaiAmoled = staticCompositionLocalOf { false }

/**
 * App theme = `MaterialExpressiveTheme` + generated token palettes (paletteId / AMOLED / contrast level)
 * + optional system dynamic color (Android 12+) + `MotionScheme.expressive()` (01 §2, §5).
 */
@Composable
fun TatakaiTheme(
    paletteId: String = TatakaiTokens.DEFAULT_PALETTE,
    amoled: Boolean = false,
    contrast: ContrastLevel = ContrastLevel.Standard,
    dynamicColor: Boolean = false,
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    val context = LocalContext.current
    val colorScheme = remember(paletteId, amoled, contrast, dynamicColor, darkTheme) {
        val useSystem = dynamicColor && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S
        if (useSystem) {
            val base = if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
            if (darkTheme && amoled) base.toAmoled() else base
        } else {
            val schemes = TatakaiTokens.palette(paletteId, contrast)
                ?: checkNotNull(TatakaiTokens.palette(TatakaiTokens.DEFAULT_PALETTE, contrast)) { "default palette missing" }
            when {
                !darkTheme -> schemes.light
                amoled -> schemes.amoled
                else -> schemes.dark
            }
        }
    }
    val typography = remember { TatakaiTokens.typography(TatakaiFonts.families) }
    CompositionLocalProvider(
        LocalTatakaiDarkTheme provides darkTheme,
        LocalTatakaiAmoled provides (darkTheme && amoled),
    ) {
        MaterialExpressiveTheme(
            colorScheme = colorScheme,
            motionScheme = MotionScheme.expressive(),
            shapes = TatakaiTokens.shapes,
            typography = typography,
            content = content,
        )
    }
}

/**
 * Per-content accent (e.g. the poster colour in Details), derived at runtime with materialKolor (01 §2).
 * When [seed] is null the surrounding theme is used unchanged. Light/dark and AMOLED follow the app's resolved mode
 * ([LocalTatakaiDarkTheme], [LocalTatakaiAmoled]), never the system's: with Modo "Oscuro" on a light system the
 * seeded scheme used to come out light and left the Details text unreadable.
 */
@Composable
fun TatakaiSeedTheme(
    seed: Color?,
    dark: Boolean = LocalTatakaiDarkTheme.current,
    amoled: Boolean = LocalTatakaiAmoled.current,
    content: @Composable () -> Unit,
) {
    if (seed == null) {
        content()
    } else {
        DynamicMaterialExpressiveTheme(
            seedColor = seed,
            motionScheme = MotionScheme.expressive(),
            isDark = dark,
            isAmoled = dark && amoled,
            style = PaletteStyle.Expressive,
            shapes = MaterialTheme.shapes,
            typography = MaterialTheme.typography,
            animate = true,
            content = content,
        )
    }
}

/** Parses `#RRGGBB` / `#AARRGGBB` (as delivered by AniList cover colours); null when malformed. */
fun parseHexColor(hex: String?): Color? {
    val s = hex?.trim()?.removePrefix("#") ?: return null
    val v = s.toLongOrNull(16) ?: return null
    return when (s.length) {
        6 -> Color(0xFF000000L or v)
        8 -> Color(v)
        else -> null
    }
}

private fun ColorScheme.toAmoled(): ColorScheme = copy(
    background = Color.Black,
    surface = Color.Black,
    surfaceDim = Color.Black,
    surfaceContainerLowest = Color.Black,
)
