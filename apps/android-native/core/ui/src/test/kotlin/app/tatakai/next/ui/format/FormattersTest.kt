package app.tatakai.next.ui.format

import kotlin.test.Test
import kotlin.test.assertEquals

class FormattersTest {

    @Test
    fun `bytes use decimal units with three significant digits like desktop`() {
        assertEquals("999 B", formatBytes(999))
        assertEquals("1.20 kB", formatBytes(1_200))
        assertEquals("38.4 MB", formatBytes(38_400_000))
        assertEquals("1.38 GB", formatBytes(1_380_000_000))
        assertEquals("0 B", formatBytes(-5))
    }

    @Test
    fun `rates and durations`() {
        assertEquals("0 B/s", formatRate(0))
        assertEquals("4.2 MB/s", formatRate(4_200_000))
        assertEquals("23:45", formatDuration(23 * 60 + 45))
        assertEquals("1:23:45", formatDuration(3600 + 23 * 60 + 45))
        assertEquals("0:00", formatDuration(-1))
    }

    @Test
    fun `relative time buckets`() {
        assertEquals(RelativeTime.Now, relativeTimeBucket(30_000))
        assertEquals(RelativeTime.Now, relativeTimeBucket(-10_000))
        assertEquals(RelativeTime.Minutes(14), relativeTimeBucket(14 * 60_000L))
        assertEquals(RelativeTime.Hours(3), relativeTimeBucket(3 * 3_600_000L + 59_000))
        assertEquals(RelativeTime.Yesterday, relativeTimeBucket(30 * 3_600_000L))
        assertEquals(RelativeTime.Days(5), relativeTimeBucket(5 * 86_400_000L))
    }
}
