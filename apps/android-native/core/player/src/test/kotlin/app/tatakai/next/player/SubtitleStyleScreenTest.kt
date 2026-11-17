// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player

import app.tatakai.next.model.AppSettings
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SubtitleAlign
import app.tatakai.next.model.SubtitleStyle
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.test.runTest
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

/** 0.4.1 (subtitle settings): 0.4.0 extras migration and the Media3 / screen-drawn mapping. */
class SubtitleStyleScreenTest {
    // ---- migration of 0.4.0's SharedPreferences extras ----

    @Test
    fun nothingStoredMeansNoMigration() {
        assertNull(LegacySubtitleExtras.from(emptyMap<String, Any>()))
        assertNull(LegacySubtitleExtras.from(mapOf("unrelated" to 1)))
    }

    @Test
    fun storedExtrasMoveIntoTheStyle() = runTest {
        val stored = mapOf(
            "sub_text_color" to SubtitleStyle.YELLOW,
            "sub_outline_color" to 0xFF1A237E.toInt(),
            "sub_outline_size" to 4f,
            "sub_position" to 75,
            "sub_font_file" to "Mine.ttf",
        )
        val legacy = LegacySubtitleExtras.from(stored)!!
        // What PlayerPrefs does: one DataStore update keeping the rest of the style (0.4.0's scale, box, bold...).
        val repo = FakeSettings(AppSettings(subtitleStyle = SubtitleStyle(scale = 1.3f, backgroundOpacity = 0.2f, bold = true)))
        repo.update { it.copy(subtitleStyle = legacy.applyTo(it.subtitleStyle)) }
        val s = repo.settings.value.subtitleStyle
        assertEquals(1.3f, s.scale)
        assertEquals(0.2f, s.backgroundOpacity)
        assertTrue(s.bold)
        assertEquals(SubtitleStyle.YELLOW, s.textColor)
        assertEquals(0xFF1A237E.toInt(), s.outlineColor)
        assertEquals(4f, s.outlineSize)
        assertEquals(75, s.positionPct)
        assertEquals("Mine.ttf", s.fontFile)
    }

    @Test
    fun wrongTypesAndBlankFontAreIgnored() {
        val legacy = LegacySubtitleExtras.from(mapOf("sub_text_color" to "white", "sub_outline_size" to Float.NaN, "sub_font_file" to " "))!!
        assertEquals(SubtitleStyle(), legacy.applyTo(SubtitleStyle()))
    }

    // ---- Media3 / screen-drawn mapping ----

    @Test
    fun sizesFollowTheVideoHeightLikeMpv() {
        val at720 = ScreenSubtitleSpec.of(SubtitleStyle(), 720f)
        assertEquals(38f, at720.fontSizeDp, 0.01f)
        assertEquals(SubtitleStyle.DEFAULT_MARGIN_Y.toFloat(), at720.marginYDp, 0.01f)
        val big = ScreenSubtitleSpec.of(SubtitleStyle(scale = 2f), 360f)
        assertEquals(38f, big.fontSizeDp, 0.01f)
        // No video area yet (preview): a phone-landscape reference instead of 0.
        assertEquals(ScreenSubtitleSpec.of(SubtitleStyle(), ScreenSubtitleSpec.PREVIEW_HEIGHT_DP), ScreenSubtitleSpec.of(SubtitleStyle(), 0f))
    }

    @Test
    fun outlineIsAStrokeTwiceAsWideAndOffWithTheSwitch() {
        val on = ScreenSubtitleSpec.of(SubtitleStyle(outlineSize = 3f), 720f)
        assertEquals(6f, on.strokeWidthDp, 0.01f)
        assertEquals(0f, ScreenSubtitleSpec.of(SubtitleStyle(outline = false, outlineSize = 3f), 720f).strokeWidthDp)
    }

    @Test
    fun boxCarriesItsOpacityAndHidesTheShadow() {
        val box = ScreenSubtitleSpec.of(SubtitleStyle(background = true, backgroundOpacity = 0.5f, backgroundColor = 0xFF1A237E.toInt(), shadowOffset = 2f), 720f)
        assertEquals(0x801A237E.toInt(), box.boxColor)
        assertEquals(0f, box.shadowOffsetDp)
        val noBox = ScreenSubtitleSpec.of(SubtitleStyle(background = false, shadowOffset = 2f, shadowColor = 0x80000000.toInt()), 720f)
        assertNull(noBox.boxColor)
        assertEquals(2f, noBox.shadowOffsetDp, 0.01f)
        assertEquals(0x80000000.toInt(), noBox.shadowColor)
        // Box on but fully transparent draws nothing.
        assertNull(ScreenSubtitleSpec.of(SubtitleStyle(background = true, backgroundOpacity = 0f), 720f).boxColor)
    }

    @Test
    fun positionAlignmentColoursAndSpacing() {
        val s = ScreenSubtitleSpec.of(
            SubtitleStyle(positionPct = 50, align = SubtitleAlign.LEFT, textColor = 0x80FFFFFF.toInt(), italic = true, lineSpacing = 10f, blur = 2f, marginX = 50),
            720f,
        )
        assertEquals(0.4f, s.liftFraction, 0.001f)
        assertEquals(SubtitleAlign.LEFT, s.align)
        assertEquals(0x80FFFFFF.toInt(), s.textColor)
        assertTrue(s.italic)
        assertEquals(38f * 1.25f + 10f, s.lineHeightDp, 0.01f)
        assertEquals(2f, s.blurDp, 0.01f)
        assertEquals(50f, s.marginXDp, 0.01f)
        assertEquals(0f, ScreenSubtitleSpec.of(SubtitleStyle(positionPct = 100), 720f).liftFraction)
    }

    private class FakeSettings(initial: AppSettings) : SettingsRepository {
        override val settings = MutableStateFlow(initial)

        override suspend fun update(transform: (AppSettings) -> AppSettings) {
            settings.value = transform(settings.value)
        }
    }
}
