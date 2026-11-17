// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.data

import androidx.datastore.preferences.core.mutablePreferencesOf
import androidx.datastore.preferences.core.stringPreferencesKey
import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.AssStyleMode
import app.tatakai.next.model.SubtitleAlign
import app.tatakai.next.model.SubtitleStyle
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** 0.4.1 (subtitle settings): the full subtitle style in `subtitle_style_json`, and 0.4.0's stored styles reading the same. */
class SubtitleStylePersistenceTest {
    private val key = stringPreferencesKey("subtitle_style_json")

    private fun roundTrip(style: SubtitleStyle): SubtitleStyle {
        val p = mutablePreferencesOf()
        SettingsCodec.write(p, AppSettings(subtitleStyle = style))
        return SettingsCodec.read(p).subtitleStyle
    }

    @Test
    fun everyNewFieldSurvivesAWriteAndRead() {
        val style = SubtitleStyle(
            scale = 1.35f,
            backgroundOpacity = 0.25f,
            font = SubtitleStyle.FONT_SPACE_GROTESK,
            bold = true,
            outline = false,
            italic = true,
            textColor = 0xCCFFEB3B.toInt(),
            outlineColor = 0xFF1A237E.toInt(),
            outlineSize = 4.5f,
            background = false,
            backgroundColor = 0xFF3E2723.toInt(),
            shadowOffset = 2f,
            shadowColor = 0x40000000,
            positionPct = 85,
            marginX = 40,
            marginY = 60,
            lineSpacing = 6f,
            blur = 1.5f,
            align = SubtitleAlign.LEFT,
            fontFile = "MyFont.ttf",
            assMode = AssStyleMode.MINE,
            assScale = true,
        )
        assertEquals(style, roundTrip(style))
    }

    @Test
    fun aStoredStyleFrom040KeepsItsBox() {
        // 0.4.0 wrote only the five old fields; no "background" key.
        val p = mutablePreferencesOf(key to """{"scale":1.3,"backgroundOpacity":0.4,"font":"system","bold":true,"outline":true}""")
        val read = SettingsCodec.read(p).subtitleStyle
        assertTrue(read.background)
        assertEquals(0.4f, read.backgroundOpacity)
        assertEquals(1.3f, read.scale)
        assertEquals(SubtitleStyle.FONT_SYSTEM, read.font)
        assertTrue(read.bold)
        // New fields take their defaults (0.4.0's look).
        assertEquals(SubtitleStyle.WHITE, read.textColor)
        assertEquals(100, read.positionPct)
        assertEquals(AssStyleMode.ORIGINAL, read.assMode)
    }

    @Test
    fun aStoredStyleFrom040WithoutBoxTurnsTheSwitchOff() {
        val p = mutablePreferencesOf(key to """{"scale":1.0,"backgroundOpacity":0.0,"font":"outfit","bold":false,"outline":true}""")
        val read = SettingsCodec.read(p).subtitleStyle
        assertFalse(read.background)
        // The default opacity is ready for when the user turns the box on.
        assertEquals(SubtitleStyle().backgroundOpacity, read.backgroundOpacity)
    }

    @Test
    fun outOfRangeValuesAreClampedAndUnknownEnumsFallBack() {
        val p = mutablePreferencesOf(
            key to """{"scale":0.1,"background":true,"outlineSize":40,"shadowOffset":-3,"positionPct":300,"marginX":-5,"marginY":9999,""" +
                """"lineSpacing":99,"blur":50,"align":"diagonal","assMode":"wild","fontFile":"  "}""",
        )
        val read = SettingsCodec.read(p).subtitleStyle
        assertEquals(SubtitleStyle.MIN_SCALE, read.scale)
        assertEquals(SubtitleStyle.MAX_OUTLINE, read.outlineSize)
        assertEquals(0f, read.shadowOffset)
        assertEquals(100, read.positionPct)
        assertEquals(0, read.marginX)
        assertEquals(SubtitleStyle.MAX_MARGIN, read.marginY)
        assertEquals(SubtitleStyle.MAX_LINE_SPACING, read.lineSpacing)
        assertEquals(SubtitleStyle.MAX_BLUR, read.blur)
        assertEquals(SubtitleAlign.CENTER, read.align)
        assertEquals(AssStyleMode.ORIGINAL, read.assMode)
        assertEquals(null, read.fontFile)
    }
}
