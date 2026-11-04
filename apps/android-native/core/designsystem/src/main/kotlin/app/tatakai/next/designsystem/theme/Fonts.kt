@file:OptIn(ExperimentalTextApi::class)

package app.tatakai.next.designsystem.theme

import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import app.tatakai.next.designsystem.R
import app.tatakai.next.designsystem.generated.TatakaiFontFamilies

/**
 * Font families used by the type scale (01 §3): Space Grotesk for display/headline/title, Outfit for body/label,
 * JetBrains Mono for figures and monospace.
 *
 * The files in `res/font/` are the variable `[wght]` builds from github.com/google/fonts (SIL OFL 1.1; licenses in
 * `assets/licenses/OFL-*.txt`, rows in THIRD_PARTY_NOTICES.md §3). One variable file serves every weight: each [Font] entry
 * pins the `wght` axis through `FontVariation.Settings(weight, style)` (compose-ui `Font(resId, weight, style, loadingStrategy,
 * variationSettings)`, `@ExperimentalTextApi`, verified with javap on ui-text-android 1.11.4; minSdk 26 supports variation axes).
 */
object TatakaiFonts {
    private val WEIGHTS = listOf(FontWeight.W400, FontWeight.W500, FontWeight.W600, FontWeight.W700)

    private fun variable(resId: Int, weights: List<FontWeight> = WEIGHTS): FontFamily = FontFamily(
        weights.map { w ->
            Font(
                resId = resId,
                weight = w,
                style = FontStyle.Normal,
                variationSettings = FontVariation.Settings(w, FontStyle.Normal),
            )
        },
    )

    val spaceGrotesk: FontFamily = variable(R.font.space_grotesk)
    val outfit: FontFamily = variable(R.font.outfit)
    val jetBrainsMono: FontFamily = variable(R.font.jetbrains_mono)

    val families: TatakaiFontFamilies = TatakaiFontFamilies(
        display = spaceGrotesk,
        text = outfit,
        mono = jetBrainsMono,
    )
}
