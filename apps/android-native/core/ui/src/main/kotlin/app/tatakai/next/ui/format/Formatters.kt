// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.format

import java.util.Locale

// android wave contract (docs/redesign/12 §3.9). Shared technical formatters (same output as desktop: decimal units, "1,38 GB",
// "4,2 MB/s" with the UI language's separator); units are not translated. Implemented here (contract); owner afterwards: worker b. Use the mono font for figures.

private val UNITS = arrayOf("B", "kB", "MB", "GB", "TB")

/**
 * Decimal units, 3 significant digits: 999 B, 1.20 kB, 38.4 MB, 1.38 GB. BUG-074: the decimal separator is the UI language's ("1,38 GB"
 * in Spanish, next to "92 %"), as on the desktop.
 */
fun formatBytes(bytes: Long, locale: Locale = Locale.getDefault()): String {
    if (bytes < 1000) return "${bytes.coerceAtLeast(0)} B"
    var v = bytes.toDouble()
    var u = 0
    while (v >= 1000 && u < UNITS.lastIndex) {
        v /= 1000
        u++
    }
    val pattern = when {
        v >= 100 -> "%.0f %s"
        v >= 10 -> "%.1f %s"
        else -> "%.2f %s"
    }
    return String.format(locale, pattern, v, UNITS[u])
}

/** Transfer rate: "4.2 MB/s" ("4,2 MB/s" in Spanish, BUG-074), "0 B/s". */
fun formatRate(bytesPerSec: Long, locale: Locale = Locale.getDefault()): String {
    if (bytesPerSec < 1000) return "${bytesPerSec.coerceAtLeast(0)} B/s"
    var v = bytesPerSec.toDouble()
    var u = 0
    while (v >= 1000 && u < UNITS.lastIndex) {
        v /= 1000
        u++
    }
    return String.format(locale, if (v >= 100) "%.0f %s/s" else "%.1f %s/s", v, UNITS[u])
}

/** "1:23:45" or "23:45". Negative -> "0:00". */
fun formatDuration(totalSecs: Long): String {
    val s = totalSecs.coerceAtLeast(0)
    val h = s / 3600
    val m = (s % 3600) / 60
    val sec = s % 60
    return if (h > 0) String.format(Locale.ROOT, "%d:%02d:%02d", h, m, sec) else String.format(Locale.ROOT, "%d:%02d", m, sec)
}
