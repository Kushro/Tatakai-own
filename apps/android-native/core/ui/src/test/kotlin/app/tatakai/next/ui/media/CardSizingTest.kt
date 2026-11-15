// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.media

import app.tatakai.next.model.CardSize
import app.tatakai.next.ui.look.Ago
import app.tatakai.next.ui.look.CardSizing
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class CardSizingTest {
    @Test
    fun posterSizes() {
        assertEquals(120f, CardSizing.posterDp(CardSize.MEDIUM, compact = false))
        assertEquals(96f, CardSizing.posterDp(CardSize.SMALL, compact = false))
        assertEquals(120f * 0.87f, CardSizing.posterDp(CardSize.MEDIUM, compact = true))
    }

    @Test
    fun phonesShowAboutOnePointThreeLargeAndOneXlCard() {
        for (phone in listOf(360f, 393f, 411f, 430f)) {
            val large = CardSizing.wideCardDp(CardSize.LARGE, phone)
            val xl = CardSizing.wideCardDp(CardSize.XL, phone)
            val visLarge = CardSizing.visibleCards(phone, large)
            val visXl = CardSizing.visibleCards(phone, xl)
            assertTrue(visLarge in 1.2f..1.4f, "large on $phone dp: $visLarge cards")
            assertTrue(visXl in 1.0f..1.15f, "xl on $phone dp: $visXl cards")
            assertTrue(xl > large)
        }
    }

    @Test
    fun wideWindowsUseFixedWidths() {
        assertEquals(320f, CardSizing.wideCardDp(CardSize.LARGE, 840f))
        assertEquals(420f, CardSizing.wideCardDp(CardSize.XL, 1280f))
        // about 3-5 large cards on tablets / desktop mode
        assertTrue(CardSizing.visibleCards(1280f, 320f) in 3f..5f)
    }

    @Test
    fun agoBuckets() {
        assertEquals(Ago.Now, Ago.of(-5))
        assertEquals(Ago.Now, Ago.of(59))
        assertEquals(Ago.Minutes(5), Ago.of(5 * 60 + 10))
        assertEquals(Ago.Hours(3), Ago.of(3 * 3_600 + 59))
        assertEquals(Ago.Days(2), Ago.of(2 * 86_400 + 1))
    }
}
