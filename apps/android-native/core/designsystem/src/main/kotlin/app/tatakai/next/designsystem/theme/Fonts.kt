package app.tatakai.next.designsystem.theme

import androidx.compose.ui.text.font.FontFamily
import app.tatakai.next.designsystem.generated.TatakaiFontFamilies

/**
 * Font families used by the type scale (01 §3): Space Grotesk for display/headline/title, Outfit for body/label,
 * JetBrains Mono for monospace.
 *
 * TODO(fonts): the OFL font files are NOT bundled yet (downloading them needs user approval; none exist in the local repos).
 * Drop `space_grotesk_*.ttf`, `outfit_*.ttf`, `jetbrains_mono_*.ttf` + their `OFL.txt` into `core/designsystem/src/main/res/font/`
 * and replace the families below with `FontFamily(Font(R.font.<name>, FontWeight.<w>), ...)`.
 * Until then the platform default family is used (layout is unaffected: sizes/line heights come from the tokens).
 */
object TatakaiFonts {
    val families: TatakaiFontFamilies = TatakaiFontFamilies(
        display = FontFamily.Default,
        text = FontFamily.Default,
        mono = FontFamily.Monospace,
    )
}
