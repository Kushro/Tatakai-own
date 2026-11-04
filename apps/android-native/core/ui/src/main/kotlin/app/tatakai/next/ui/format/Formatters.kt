// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.format

import java.util.Locale

// android wave contract (docs/redesign/12 §3.9). Shared technical formatters (same output as desktop: decimal units, "1.38 GB",
// "4.2 MB/s"); units are not translated. Implemented here (contract); owner afterwards: worker b. Use the mono font for figures.

private val UNITS = arrayOf("B", "kB", "MB", "GB", "TB")

/** Decimal units, 3 significant digits: 999 B, 1.20 kB, 38.4 MB, 1.38 GB. */
fun formatBytes(bytes: Long): String {
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
    return String.format(Locale.ROOT, pattern, v, UNITS[u])
}

/** Transfer rate: "4.2 MB/s", "0 B/s". */
fun formatRate(bytesPerSec: Long): String {
    if (bytesPerSec < 1000) return "${bytesPerSec.coerceAtLeast(0)} B/s"
    var v = bytesPerSec.toDouble()
    var u = 0
    while (v >= 1000 && u < UNITS.lastIndex) {
        v /= 1000
        u++
    }
    return String.format(Locale.ROOT, if (v >= 100) "%.0f %s/s" else "%.1f %s/s", v, UNITS[u])
}

/** "1:23:45" or "23:45". Negative -> "0:00". */
fun formatDuration(totalSecs: Long): String {
    val s = totalSecs.coerceAtLeast(0)
    val h = s / 3600
    val m = (s % 3600) / 60
    val sec = s % 60
    return if (h > 0) String.format(Locale.ROOT, "%d:%02d:%02d", h, m, sec) else String.format(Locale.ROOT, "%d:%02d", m, sec)
}
