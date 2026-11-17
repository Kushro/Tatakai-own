// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.model

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertFalse
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SubtitleStyleTest {
    @Test
    fun defaultsAre040sLook() {
        val d = SubtitleStyle()
        assertEquals(1f, d.scale)
        assertEquals(0.6f, d.backgroundOpacity)
        assertTrue(d.background)
        assertEquals(SubtitleStyle.FONT_OUTFIT, d.font)
        assertEquals(SubtitleStyle.WHITE, d.textColor)
        assertEquals(SubtitleStyle.BLACK, d.outlineColor)
        assertEquals(2.5f, d.outlineSize)
        assertEquals(100, d.positionPct)
        assertEquals(AssStyleMode.ORIGINAL, d.assMode)
        assertFalse(d.assScale)
    }

    @Test
    fun sanitizedClampsAndReplacesNaN() {
        val s = SubtitleStyle(scale = Float.NaN, backgroundOpacity = 3f, font = "comic", outlineSize = -1f, blur = Float.POSITIVE_INFINITY, fontFile = " ").sanitized()
        assertEquals(1f, s.scale)
        assertEquals(1f, s.backgroundOpacity)
        assertEquals(SubtitleStyle.FONT_OUTFIT, s.font)
        assertEquals(0f, s.outlineSize)
        assertEquals(0f, s.blur)
        assertNull(s.fontFile)
    }

    @Test
    fun effectiveOutlineAndShadowFollowTheSwitches() {
        assertEquals(0f, SubtitleStyle(outline = false, outlineSize = 4f).effectiveOutline)
        assertEquals(4f, SubtitleStyle(outline = true, outlineSize = 4f).effectiveOutline)
        // libass paints the box with the shadow colour: no separate shadow under the box.
        assertEquals(0f, SubtitleStyle(background = true, shadowOffset = 2f).effectiveShadow)
        assertEquals(2f, SubtitleStyle(background = false, shadowOffset = 2f).effectiveShadow)
    }

    @Test
    fun withAlphaReplacesOnlyTheAlphaByte() {
        assertEquals(0x80FFEB3B.toInt(), SubtitleStyle.withAlpha(SubtitleStyle.YELLOW, 0.5f))
        assertEquals(0x00FFFFFF, SubtitleStyle.withAlpha(SubtitleStyle.WHITE, 0f))
        assertEquals(SubtitleStyle.BLACK, SubtitleStyle.withAlpha(0x00000000, 1f))
        assertEquals(0.5f, SubtitleStyle(textColor = 0x80FFFFFF.toInt()).textOpacity, 0.01f)
    }

    @Test
    fun presetsChangeTheLookButKeepSizePositionFontAndAss() {
        val mine = SubtitleStyle(scale = 1.6f, positionPct = 70, marginY = 50, fontFile = "x.ttf", assMode = AssStyleMode.PLAIN, align = SubtitleAlign.RIGHT)
        val boxed = mine.withPreset(SubtitlePreset.BOX)
        assertEquals(1.6f, boxed.scale)
        assertEquals(70, boxed.positionPct)
        assertEquals(50, boxed.marginY)
        assertEquals("x.ttf", boxed.fontFile)
        assertEquals(AssStyleMode.PLAIN, boxed.assMode)
        assertEquals(SubtitleAlign.RIGHT, boxed.align)
        assertTrue(boxed.background)
        assertFalse(boxed.outline)
        val yellow = mine.withPreset(SubtitlePreset.YELLOW)
        assertEquals(SubtitleStyle.YELLOW, yellow.textColor)
        assertFalse(yellow.background)
    }

    @Test
    fun legacyExtrasFoldIn() {
        val base = SubtitleStyle(scale = 1.2f, bold = true)
        val migrated = base.withLegacyExtras(textColor = SubtitleStyle.YELLOW, outlineColor = 0xFF4A148C.toInt(), outlineSize = 9f, positionPct = 80, fontFile = "Old.otf")
        assertEquals(1.2f, migrated.scale)
        assertTrue(migrated.bold)
        assertEquals(SubtitleStyle.YELLOW, migrated.textColor)
        assertEquals(0xFF4A148C.toInt(), migrated.outlineColor)
        assertEquals(SubtitleStyle.MAX_OUTLINE, migrated.outlineSize)
        assertEquals(80, migrated.positionPct)
        assertEquals("Old.otf", migrated.fontFile)
        // Nothing stored: nothing changes.
        assertEquals(base, base.withLegacyExtras())
    }
}
