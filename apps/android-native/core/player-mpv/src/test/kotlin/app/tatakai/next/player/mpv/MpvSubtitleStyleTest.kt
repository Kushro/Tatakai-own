// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import app.tatakai.next.model.AssStyleMode
import app.tatakai.next.model.SubtitleAlign
import app.tatakai.next.model.SubtitleStyle
import kotlin.test.Test
import kotlin.test.assertEquals

/** 0.4.1 (subtitle settings): `SubtitleStyle` -> libmpv `sub-*` options, and the record-if-changed applier (from Enigmata). */
class MpvSubtitleStyleTest {
    @Test
    fun defaultsMatch040sLook() {
        val p = mpvSubtitleProperties(SubtitleStyle(), "Outfit")
        assertEquals("Outfit", p["sub-font"])
        assertEquals("no", p["sub-bold"])
        assertEquals("no", p["sub-italic"])
        assertEquals("center", p["sub-justify"])
        assertEquals("no", p["sub-ass-justify"])
        assertEquals("background-box", p["sub-border-style"])
        assertEquals("2.50", p["sub-outline-size"])
        assertEquals("0.00", p["sub-shadow-offset"])
        assertEquals("#FFFFFFFF", p["sub-color"])
        assertEquals("#FF000000", p["sub-outline-color"])
        // Box: black at 60 % (0x99).
        assertEquals("#99000000", p["sub-back-color"])
        assertEquals("25", p["sub-margin-x"])
        assertEquals("22", p["sub-margin-y"])
        assertEquals("100", p["sub-pos"])
        assertEquals("1.00", p["sub-scale"])
        assertEquals("no", p["sub-ass-override"])
        assertEquals("no", p["secondary-sub-ass-override"])
    }

    @Test
    fun withoutTheBoxTheBackColourIsTheShadow() {
        val p = mpvSubtitleProperties(SubtitleStyle(background = false, shadowOffset = 1.5f, shadowColor = 0x80102030.toInt()), "x")
        assertEquals("outline-and-shadow", p["sub-border-style"])
        assertEquals("1.50", p["sub-shadow-offset"])
        assertEquals("#80102030", p["sub-back-color"])
    }

    @Test
    fun boxTransparencyAndColour() {
        val p = mpvSubtitleProperties(SubtitleStyle(background = true, backgroundOpacity = 0.25f, backgroundColor = 0xFF1A237E.toInt(), shadowOffset = 3f), "x")
        assertEquals("#401A237E", p["sub-back-color"])
        assertEquals("0.00", p["sub-shadow-offset"], "no separate shadow under the box")
        // A fully transparent box is no box.
        assertEquals("outline-and-shadow", mpvSubtitleProperties(SubtitleStyle(background = true, backgroundOpacity = 0f), "x")["sub-border-style"])
    }

    @Test
    fun outlineSwitchTypographyAndLayout() {
        val p = mpvSubtitleProperties(
            SubtitleStyle(
                outline = false,
                outlineSize = 4f,
                bold = true,
                italic = true,
                align = SubtitleAlign.RIGHT,
                textColor = 0xCCFFEB3B.toInt(),
                positionPct = 80,
                marginX = 40,
                marginY = 60,
                scale = 1.35f,
                blur = 1.25f,
                lineSpacing = 4f,
            ),
            "Space Grotesk",
            screenPxPerUnit = 1.5f,
        )
        assertEquals("0.00", p["sub-outline-size"])
        assertEquals("yes", p["sub-bold"])
        assertEquals("yes", p["sub-italic"])
        assertEquals("right", p["sub-justify"])
        assertEquals("#CCFFEB3B", p["sub-color"])
        assertEquals("80", p["sub-pos"])
        assertEquals("40", p["sub-margin-x"])
        assertEquals("60", p["sub-margin-y"])
        assertEquals("1.35", p["sub-scale"])
        assertEquals("1.25", p["sub-blur"])
        // libass does not scale line spacing: 4 scaled px on a 1080 px tall screen = 6 px.
        assertEquals("6.00", p["sub-line-spacing"])
    }

    @Test
    fun assModes() {
        fun ass(s: SubtitleStyle) = mpvSubtitleProperties(s, "x").let { it["sub-ass-override"] to it["secondary-sub-ass-override"] }
        assertEquals("no" to "no", ass(SubtitleStyle(assMode = AssStyleMode.ORIGINAL)))
        assertEquals("scale" to "scale", ass(SubtitleStyle(assMode = AssStyleMode.ORIGINAL, assScale = true)))
        assertEquals("force" to "force", ass(SubtitleStyle(assMode = AssStyleMode.MINE)))
        assertEquals("strip" to "strip", ass(SubtitleStyle(assMode = AssStyleMode.PLAIN, assScale = true)))
        assertEquals("yes", mpvSubtitleProperties(SubtitleStyle(assMode = AssStyleMode.MINE), "x")["sub-ass-justify"])
    }

    @Test
    fun outOfRangeValuesAreClampedBeforeMpvSeesThem() {
        val p = mpvSubtitleProperties(SubtitleStyle(scale = 9f, positionPct = -20, outlineSize = 50f, marginY = 5000), "x")
        assertEquals("2.50", p["sub-scale"])
        assertEquals("0", p["sub-pos"])
        assertEquals("6.00", p["sub-outline-size"])
        assertEquals("150", p["sub-margin-y"])
    }

    @Test
    fun fontFamilies() {
        assertEquals("Outfit", mpvFontFamily(SubtitleStyle(font = SubtitleStyle.FONT_OUTFIT), null))
        assertEquals("Space Grotesk", mpvFontFamily(SubtitleStyle(font = SubtitleStyle.FONT_SPACE_GROTESK), null))
        assertEquals(SYSTEM_SUB_FONT, mpvFontFamily(SubtitleStyle(font = SubtitleStyle.FONT_SYSTEM), null))
        assertEquals("My Font", mpvFontFamily(SubtitleStyle(fontFile = "my.ttf"), "My Font"))
        assertEquals("Outfit", mpvFontFamily(SubtitleStyle(fontFile = "gone.ttf"), null), "missing imported file: the bundled font")
    }

    @Test
    fun applierWritesOnlyWhatChanged() {
        val written = mutableListOf<Pair<String, String>>()
        val applier = MpvSubtitleStyleApplier { n, v -> written += n to v }
        val first = mpvSubtitleProperties(SubtitleStyle(), "Outfit")
        assertEquals(first.size, applier.apply(first))
        assertEquals(first.keys.toList(), written.map { it.first }, "write order is the map's order")
        written.clear()
        assertEquals(0, applier.apply(first))
        // Dragging the size slider sends one property.
        assertEquals(1, applier.apply(mpvSubtitleProperties(SubtitleStyle(scale = 1.2f), "Outfit")))
        assertEquals(listOf("sub-scale" to "1.20"), written)
        applier.invalidate()
        written.clear()
        assertEquals(first.size, applier.apply(first))
    }
}
