// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import java.util.Locale
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class AirDateTest {
    @Test
    fun isoInstantAndPlainDateAreLocalized() {
        val en = formatAirDate("1999-10-20T14:15:00Z", Locale.US)
        assertTrue(en.contains("1999") && en.contains("Oct"), en)
        assertEquals(en, formatAirDate("1999-10-20", Locale.US))
    }

    @Test
    fun unparsableValuesAreShownAsTheyCame() {
        assertEquals("TBA", formatAirDate("TBA"))
        assertEquals("", formatAirDate(""))
    }
}
