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
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import app.tatakai.next.designsystem.generated.ContrastLevel
import app.tatakai.next.designsystem.generated.TatakaiTokens
import com.materialkolor.DynamicMaterialExpressiveTheme
import com.materialkolor.PaletteStyle

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
    MaterialExpressiveTheme(
        colorScheme = colorScheme,
        motionScheme = MotionScheme.expressive(),
        shapes = TatakaiTokens.shapes,
        typography = typography,
        content = content,
    )
}

/**
 * Per-content accent (e.g. the poster colour in Details), derived at runtime with materialKolor (01 §2).
 * When [seed] is null the surrounding theme is used unchanged.
 */
@Composable
fun TatakaiSeedTheme(
    seed: Color?,
    amoled: Boolean = false,
    content: @Composable () -> Unit,
) {
    if (seed == null) {
        content()
    } else {
        DynamicMaterialExpressiveTheme(
            seedColor = seed,
            isAmoled = amoled,
            style = PaletteStyle.Expressive,
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
