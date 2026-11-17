// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * Subtitle look (Settings › Reproductor › Estilo de subtítulos, and the player's subtitle sheet › Estilo). Stored as JSON in the settings
 * DataStore (`subtitle_style_json`), applied live by libmpv (`sub-*` options, see `MpvSubtitleStyle` in :core:player-mpv) and by the
 * subtitles the screen draws itself (Media3, translated and secondary lines; `ScreenSubtitleSpec` in :core:player).
 *
 * 0.4.1 (subtitle settings): every field after [outline] is new. Until 0.4.0 the colours, outline size, position and imported font
 * lived in the player's private SharedPreferences (`SubtitleExtras`); `PlayerPrefs` folds them in here once ([withLegacyExtras]).
 * Sizes in "scaled pixels" follow mpv: pixels of a 720 px tall window, so they grow with the screen.
 */
@Serializable
data class SubtitleStyle(
    /** Text size multiplier ([MIN_SCALE]..[MAX_SCALE], 1.0 = 100 %): mpv `sub-scale`. */
    val scale: Float = 1f,
    /** Background box opacity 0..1 (shown to the user as transparency = 1 - opacity). Only drawn while [background] is on. */
    val backgroundOpacity: Float = 0.6f,
    /** Bundled font id: [FONT_OUTFIT] | [FONT_SPACE_GROTESK] | [FONT_SYSTEM]. [fontFile] wins when set. */
    val font: String = FONT_OUTFIT,
    val bold: Boolean = false,
    /** Outline on / off (off = [outlineSize] is not drawn, the size is kept for when it comes back). */
    val outline: Boolean = true,
    // --- 0.4.1 ---
    val italic: Boolean = false,
    /** Text colour, ARGB: the alpha byte is the text opacity. */
    val textColor: Int = WHITE,
    /** Outline colour, ARGB. */
    val outlineColor: Int = BLACK,
    /** Outline width in scaled pixels, 0..[MAX_OUTLINE] (mpv `sub-outline-size`). */
    val outlineSize: Float = 2.5f,
    /** Background box behind the text (mpv `sub-border-style=background-box`). */
    val background: Boolean = true,
    /** Box colour, RGB (its alpha byte is ignored: [backgroundOpacity] is the opacity). */
    val backgroundColor: Int = BLACK,
    /** Shadow distance in scaled pixels, 0..[MAX_SHADOW]; 0 = no shadow. Without the box only (libass uses one colour for both). */
    val shadowOffset: Float = 0f,
    /** Shadow colour, ARGB (alpha = shadow opacity). */
    val shadowColor: Int = SHADOW_DEFAULT,
    /** Vertical position: mpv `sub-pos`, 100 = bottom, 0 = top. */
    val positionPct: Int = 100,
    /** Side margin in scaled pixels (mpv `sub-margin-x`). */
    val marginX: Int = DEFAULT_MARGIN_X,
    /** Bottom margin in scaled pixels (mpv `sub-margin-y`). */
    val marginY: Int = DEFAULT_MARGIN_Y,
    /** Extra space between lines in scaled pixels, 0..[MAX_LINE_SPACING]. */
    val lineSpacing: Float = 0f,
    /** Edge softness (mpv `sub-blur`), 0..[MAX_BLUR]. */
    val blur: Float = 0f,
    /** How the lines of one subtitle line up (mpv `sub-justify`). */
    val align: SubtitleAlign = SubtitleAlign.CENTER,
    /** File name of an imported font in `filesDir/fonts` (`SubtitleFonts`); null = [font]. */
    val fontFile: String? = null,
    /** ASS / SSA subtitles: keep their own styles, or apply this one (libmpv only; Media3 always draws them as plain text). */
    val assMode: AssStyleMode = AssStyleMode.ORIGINAL,
    /** With [AssStyleMode.ORIGINAL]: still apply [scale] to ASS subtitles (mpv `sub-ass-override=scale`). */
    val assScale: Boolean = false,
) {
    /** Clamps every number into its range and drops unknown font ids (NaN reads as the default). */
    fun sanitized(): SubtitleStyle {
        val d = SubtitleStyle()
        return copy(
            scale = scale.finiteOr(d.scale).coerceIn(MIN_SCALE, MAX_SCALE),
            backgroundOpacity = backgroundOpacity.finiteOr(d.backgroundOpacity).coerceIn(0f, 1f),
            font = font.takeIf { it in FONTS } ?: FONT_OUTFIT,
            outlineSize = outlineSize.finiteOr(d.outlineSize).coerceIn(0f, MAX_OUTLINE),
            shadowOffset = shadowOffset.finiteOr(d.shadowOffset).coerceIn(0f, MAX_SHADOW),
            positionPct = positionPct.coerceIn(0, 100),
            marginX = marginX.coerceIn(0, MAX_MARGIN),
            marginY = marginY.coerceIn(0, MAX_MARGIN),
            lineSpacing = lineSpacing.finiteOr(d.lineSpacing).coerceIn(0f, MAX_LINE_SPACING),
            blur = blur.finiteOr(d.blur).coerceIn(0f, MAX_BLUR),
            fontFile = fontFile?.trim()?.takeIf { it.isNotEmpty() },
        )
    }

    /** Outline width actually drawn (0 while [outline] is off). */
    val effectiveOutline: Float get() = if (outline) outlineSize else 0f

    /** Shadow actually drawn: libass paints the box with the shadow colour, so there is no separate shadow under the box. */
    val effectiveShadow: Float get() = if (background) 0f else shadowOffset

    /** Text opacity 0..1 (alpha of [textColor]). */
    val textOpacity: Float get() = ((textColor ushr 24) and 0xFF) / 255f

    /** Applies [preset]'s look (colours, outline, box, shadow, blur) and keeps size, position, margins, font and the ASS choice. */
    fun withPreset(preset: SubtitlePreset): SubtitleStyle {
        val p = preset.style
        return copy(
            textColor = p.textColor,
            outline = p.outline,
            outlineColor = p.outlineColor,
            outlineSize = p.outlineSize,
            background = p.background,
            backgroundColor = p.backgroundColor,
            backgroundOpacity = p.backgroundOpacity,
            shadowOffset = p.shadowOffset,
            shadowColor = p.shadowColor,
            blur = p.blur,
            bold = p.bold,
            italic = p.italic,
        )
    }

    /**
     * 0.4.1 migration: the 0.4.0 private extras (`PlayerPrefs` SharedPreferences `sub_text_color`, `sub_outline_color`,
     * `sub_outline_size`, `sub_position`, `sub_font_file`). Null arguments were never stored and keep this style's value.
     */
    fun withLegacyExtras(
        textColor: Int? = null,
        outlineColor: Int? = null,
        outlineSize: Float? = null,
        positionPct: Int? = null,
        fontFile: String? = null,
    ): SubtitleStyle = copy(
        textColor = textColor ?: this.textColor,
        outlineColor = outlineColor ?: this.outlineColor,
        outlineSize = outlineSize ?: this.outlineSize,
        positionPct = positionPct ?: this.positionPct,
        fontFile = fontFile ?: this.fontFile,
    ).sanitized()

    companion object {
        const val FONT_OUTFIT = "outfit"
        const val FONT_SPACE_GROTESK = "space_grotesk"
        const val FONT_SYSTEM = "system"
        val FONTS: List<String> = listOf(FONT_OUTFIT, FONT_SPACE_GROTESK, FONT_SYSTEM)

        const val MIN_SCALE = 0.5f
        const val MAX_SCALE = 2.5f
        const val MAX_OUTLINE = 6f
        const val MAX_SHADOW = 6f
        const val MAX_MARGIN = 150
        const val MAX_LINE_SPACING = 20f
        const val MAX_BLUR = 5f

        /** mpv's own defaults (`sub-margin-x` / `sub-margin-y`): what 0.4.0 drew. */
        const val DEFAULT_MARGIN_X = 25
        const val DEFAULT_MARGIN_Y = 22

        const val WHITE: Int = -0x1 // 0xFFFFFFFF
        const val BLACK: Int = -0x1000000 // 0xFF000000
        const val SHADOW_DEFAULT: Int = Int.MIN_VALUE // 0x80000000, black at 50 %
        const val YELLOW: Int = -0x14c5 // 0xFFFFEB3B

        /** Swatches of the colour pickers (ARGB, opaque); each picker adds an opacity slider. */
        val TEXT_COLORS: List<Int> = listOf(WHITE, YELLOW, 0xFF80DEEA.toInt(), 0xFFA5D6A7.toInt(), 0xFFF48FB1.toInt(), 0xFFFFCC80.toInt(), BLACK)
        val OUTLINE_COLORS: List<Int> = listOf(BLACK, 0xFF424242.toInt(), 0xFF1A237E.toInt(), 0xFF4A148C.toInt(), 0xFF004D40.toInt(), WHITE)
        val BACKGROUND_COLORS: List<Int> = listOf(BLACK, 0xFF212121.toInt(), 0xFF1A237E.toInt(), 0xFF3E2723.toInt(), WHITE)

        /** ARGB [color] with its alpha byte replaced by [alpha] 0..1. */
        fun withAlpha(color: Int, alpha: Float): Int = ((alpha.coerceIn(0f, 1f) * 255f + 0.5f).toInt() shl 24) or (color and 0xFFFFFF)

        private fun Float.finiteOr(default: Float): Float = if (isFinite()) this else default
    }
}

/** mpv `sub-justify` values. */
@Serializable
enum class SubtitleAlign(val mpv: String) {
    @SerialName("left") LEFT("left"),
    @SerialName("center") CENTER("center"),
    @SerialName("right") RIGHT("right"),
}

/**
 * ASS / SSA subtitles on libmpv: [ORIGINAL] keeps the file's styles (signs, karaoke, positions; `sub-ass-override=no`, or `scale` with
 * [SubtitleStyle.assScale]); [MINE] applies the user's style to the dialogue and keeps positioned signs (`force`); [PLAIN] removes every
 * ASS tag and style and draws plain text with the user's style (`strip`).
 */
@Serializable
enum class AssStyleMode {
    @SerialName("original") ORIGINAL,
    @SerialName("mine") MINE,
    @SerialName("plain") PLAIN,
}

/** One-tap looks of the full editor (they keep size, position, margins, font and the ASS choice). */
enum class SubtitlePreset(val id: String, val style: SubtitleStyle) {
    /** 0.4.0's default: white text, thin outline, translucent dark box. */
    DEFAULT("default", SubtitleStyle()),

    /** Classic film subtitles: no box, thicker outline and a soft shadow. */
    OUTLINE(
        "outline",
        SubtitleStyle(background = false, outlineSize = 3f, shadowOffset = 1.5f),
    ),

    /** Reading comfort: almost opaque dark box, no outline. */
    BOX(
        "box",
        SubtitleStyle(background = true, backgroundOpacity = 0.85f, outline = false, outlineSize = 0f),
    ),

    /** Yellow text with a black outline, no box. */
    YELLOW(
        "yellow",
        SubtitleStyle(textColor = SubtitleStyle.YELLOW, background = false, outlineSize = 3f, shadowOffset = 1f),
    ),
}
