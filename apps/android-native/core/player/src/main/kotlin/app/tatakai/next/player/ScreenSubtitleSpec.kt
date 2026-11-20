// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.SubtitleAlign
import app.tatakai.next.model.SubtitleStyle

/**
 * 0.4.1: how the subtitles the screen draws itself (every Media3 cue, the translated line, the secondary track and the Settings preview)
 * follow [SubtitleStyle], in dp for a player whose short side is [ScreenSubtitleSpec.of] `viewportHeightDp` (its height in
 * landscape; in portrait the width, so lines keep their landscape size instead of growing with the tall screen). Pure, so the Media3 mapping is
 * unit-tested; `SubtitleLines` (:feature:player) turns it into Compose text.
 *
 * Sizes copy libmpv's rule (scaled pixels of a 720 px tall window, `sub-font-size` 38), so both engines draw the same size on the same
 * screen. Media3 has no ASS renderer: ASS cues arrive as plain text and always get this style.
 */
data class ScreenSubtitleSpec(
    val fontSizeDp: Float,
    val lineHeightDp: Float,
    /** ARGB, alpha = text opacity. */
    val textColor: Int,
    val bold: Boolean,
    val italic: Boolean,
    /** Width of the stroke drawn under the text (twice the outline: the fill covers the inner half); 0 = no outline. */
    val strokeWidthDp: Float,
    val outlineColor: Int,
    /** Softness of the outline (or of the text without outline); 0 = sharp. */
    val blurDp: Float,
    /** Shadow distance (down and right); 0 = no shadow. */
    val shadowOffsetDp: Float,
    val shadowColor: Int,
    /** Box colour with its opacity (ARGB), null = no box. */
    val boxColor: Int?,
    val align: SubtitleAlign,
    /** Side padding of the block (`sub-margin-x`). */
    val marginXDp: Float,
    /** Distance from the bottom edge at position 100 (`sub-margin-y`). */
    val marginYDp: Float,
    /** Part of the free height the block is lifted by (`sub-pos` 100 = 0, 0 = most of it). */
    val liftFraction: Float,
) {
    companion object {
        /** libmpv's reference window height for "scaled pixels". */
        const val REFERENCE_HEIGHT = 720f

        /** libmpv's default `sub-font-size` (scaled pixels). */
        const val BASE_FONT_SIZE = 38f

        /** Height used when there is no video area (Settings preview): close to a phone in landscape. */
        const val PREVIEW_HEIGHT_DP = 360f

        /** sub-pos 0 must not push the block off the top; 0.4.0 drew the same 80 %. */
        private const val MAX_LIFT = 0.8f
        private const val MIN_FONT_DP = 10f

        fun of(style: SubtitleStyle, viewportHeightDp: Float): ScreenSubtitleSpec {
            val s = style.sanitized()
            val unit = (if (viewportHeightDp.isFinite() && viewportHeightDp > 0f) viewportHeightDp else PREVIEW_HEIGHT_DP) / REFERENCE_HEIGHT
            val font = (BASE_FONT_SIZE * s.scale * unit).coerceAtLeast(MIN_FONT_DP)
            // The size of the whole look follows the text size as in mpv, where sub-scale scales outline and shadow too.
            val px = unit * s.scale
            return ScreenSubtitleSpec(
                fontSizeDp = font,
                lineHeightDp = font * 1.25f + s.lineSpacing * unit,
                textColor = s.textColor,
                bold = s.bold,
                italic = s.italic,
                strokeWidthDp = s.effectiveOutline * px * 2f,
                outlineColor = s.outlineColor,
                blurDp = s.blur * px,
                shadowOffsetDp = s.effectiveShadow * px,
                shadowColor = s.shadowColor,
                boxColor = if (s.background && s.backgroundOpacity > 0f) SubtitleStyle.withAlpha(s.backgroundColor, s.backgroundOpacity) else null,
                align = s.align,
                marginXDp = s.marginX * unit,
                marginYDp = s.marginY * unit,
                liftFraction = (100 - s.positionPct) / 100f * MAX_LIFT,
            )
        }
    }
}
