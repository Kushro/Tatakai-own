// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.format

import java.time.LocalDate
import java.time.ZoneId
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull

class RelativeDayTest {
    @Test
    fun buckets() {
        assertEquals(RelativeDay.Today, relativeDayBucket(0))
        assertEquals(RelativeDay.Tomorrow, relativeDayBucket(1))
        assertEquals(RelativeDay.Yesterday, relativeDayBucket(-1))
        assertEquals(RelativeDay.Days(6), relativeDayBucket(6))
        assertEquals(RelativeDay.Days(-3), relativeDayBucket(-3))
        assertEquals(RelativeDay.Weeks(-1), relativeDayBucket(-7))
        assertEquals(RelativeDay.Weeks(4), relativeDayBucket(29))
        assertEquals(RelativeDay.Months(1), relativeDayBucket(30))
        assertEquals(RelativeDay.Months(-11), relativeDayBucket(-364))
        assertEquals(RelativeDay.Years(-2), relativeDayBucket(-800))
    }

    @Test
    fun airDays() {
        val tokyo = ZoneId.of("Asia/Tokyo")
        assertEquals(LocalDate.of(2023, 9, 29), parseAirDay("2023-09-29", tokyo))
        // 14:00 UTC is 23:00 in Tokyo (same day); 20:00 UTC is already the 30th there.
        assertEquals(LocalDate.of(2023, 9, 29), parseAirDay("2023-09-29T14:00:00Z", tokyo))
        assertEquals(LocalDate.of(2023, 9, 30), parseAirDay("2023-09-29T20:00:00Z", tokyo))
        assertEquals(LocalDate.of(2023, 9, 29), parseAirDay("2023-09-29T10:00:00+09:00", tokyo))
        assertNull(parseAirDay("soon"))
        assertNull(parseAirDay(null))
        assertEquals(-7, daysFrom(LocalDate.of(2023, 10, 6), LocalDate.of(2023, 9, 29)))
    }
}
