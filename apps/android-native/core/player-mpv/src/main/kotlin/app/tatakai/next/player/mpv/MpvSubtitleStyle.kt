// SPDX-License-Identifier: AGPL-3.0-or-later
// Adapted from Enigmata `app/src/main/java/app/enigmata/ui/player/subtitles/SubtitleStyleApplier.kt` (author's own, AGPL-3.0-or-later):
// the pure "style -> mpv property map" and the record-if-changed applier. Tatakai maps its own `SubtitleStyle` (box, shadow, margins,
// blur, line spacing, ASS modes) and applies it live instead of only on FILE_LOADED.
package app.tatakai.next.player.mpv

import app.tatakai.next.model.AssStyleMode
import app.tatakai.next.model.SubtitleStyle
import java.util.Locale

/**
 * 0.4.1: mpv property -> value for [style], in write order. [fontFamily] is the family libass looks up (`sub-font`); [screenPxPerUnit] turns
 * the style's scaled pixels into screen pixels for the only option libass does not scale itself (`sub-line-spacing`).
 *
 * - Box: `sub-border-style=background-box` paints `sub-back-color` behind the text. libass uses that same colour for the shadow, so with
 *   the box on there is no separate shadow (`sub-shadow-offset=0`); without it `sub-back-color` is the shadow colour.
 * - ASS: [AssStyleMode.ORIGINAL] = `no` (the file's styles, sub-scale ignored) or `scale` with [SubtitleStyle.assScale];
 *   [AssStyleMode.MINE] = `force`; [AssStyleMode.PLAIN] = `strip`. mpv has no secondary-sub-* look options: the secondary track takes the
 *   primary style, so only `secondary-sub-ass-override` is mirrored.
 */
fun mpvSubtitleProperties(style: SubtitleStyle, fontFamily: String, screenPxPerUnit: Float = 1f): LinkedHashMap<String, String> {
    val s = style.sanitized()
    val box = s.background && s.backgroundOpacity > 0f
    val assOverride = when (s.assMode) {
        AssStyleMode.ORIGINAL -> if (s.assScale) "scale" else "no"
        AssStyleMode.MINE -> "force"
        AssStyleMode.PLAIN -> "strip"
    }
    val unit = if (screenPxPerUnit.isFinite() && screenPxPerUnit > 0f) screenPxPerUnit else 1f
    return linkedMapOf(
        // Typography
        "sub-font" to fontFamily,
        "sub-bold" to s.bold.mpvFlag(),
        "sub-italic" to s.italic.mpvFlag(),
        "sub-justify" to s.align.mpv,
        // Our alignment also for ASS dialogue only when the user asked for their style there.
        "sub-ass-justify" to (s.assMode != AssStyleMode.ORIGINAL).mpvFlag(),
        "sub-border-style" to if (box) "background-box" else "outline-and-shadow",
        "sub-outline-size" to s.effectiveOutline.mpvNumber(),
        "sub-shadow-offset" to (if (box) 0f else s.effectiveShadow).mpvNumber(),
        "sub-blur" to s.blur.mpvNumber(),
        "sub-line-spacing" to (s.lineSpacing * unit).mpvNumber(),
        // Colours
        "sub-color" to s.textColor.mpvColor(),
        "sub-outline-color" to s.outlineColor.mpvColor(),
        "sub-back-color" to (if (box) SubtitleStyle.withAlpha(s.backgroundColor, s.backgroundOpacity) else s.shadowColor).mpvColor(),
        // Layout
        "sub-margin-x" to s.marginX.toString(),
        "sub-margin-y" to s.marginY.toString(),
        "sub-pos" to s.positionPct.toString(),
        "sub-scale" to s.scale.mpvNumber(),
        // ASS
        "sub-ass-override" to assOverride,
        "secondary-sub-ass-override" to assOverride,
    )
}

/** libass family of the style's font: the imported file's family, the bundled static copies, or Android's sans-serif. */
fun mpvFontFamily(style: SubtitleStyle, importedFamily: String?): String = importedFamily?.takeIf { it.isNotBlank() } ?: when (style.font) {
    SubtitleStyle.FONT_OUTFIT -> "Outfit"
    SubtitleStyle.FONT_SPACE_GROTESK -> "Space Grotesk"
    else -> SYSTEM_SUB_FONT
}

/** What 0.4.0 passed for "no font chosen" (libass falls back to Android's sans-serif). */
const val SYSTEM_SUB_FONT = "sans-serif"

private fun Boolean.mpvFlag(): String = if (this) "yes" else "no"

/** Two decimals, locale-independent ("2.50"; never "1.0E-4"). */
internal fun Float.mpvNumber(): String = String.format(Locale.ROOT, "%.2f", this)

/** mpv colour `#AARRGGBB` (Enigmata `toMpvColorHex`). */
internal fun Int.mpvColor(): String = "#" + String.format(Locale.ROOT, "%08X", this)

/**
 * Writes the properties of a style, only those whose value differs from the last one this applier wrote (record-if-changed, from
 * Enigmata): dragging a slider sends one property, not twenty. Invariant: [invalidate] after anything else writes one of these
 * properties (or the core restarts) so the next [apply] rewrites them all.
 */
class MpvSubtitleStyleApplier(private val writer: (name: String, value: String) -> Unit) {
    private val lastApplied = HashMap<String, String>()

    /** Writes the changed properties and returns how many were written. */
    fun apply(properties: Map<String, String>): Int {
        var written = 0
        for ((name, value) in properties) {
            if (lastApplied[name] == value) continue
            writer(name, value)
            lastApplied[name] = value
            written++
        }
        return written
    }

    /** Forgets the last applied state; the next [apply] rewrites every property. */
    fun invalidate() {
        lastApplied.clear()
    }
}
