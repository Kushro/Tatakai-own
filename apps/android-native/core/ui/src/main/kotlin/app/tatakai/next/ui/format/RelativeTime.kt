// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.format

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import app.tatakai.next.ui.R

// android wave, worker b (additive): "hace 14 min" / "14 min ago" for last sync, last seen, pairing dates.

/** Buckets of [relativeTimeBucket]: what to say about an instant [ageMs] in the past. */
sealed interface RelativeTime {
    data object Now : RelativeTime
    data class Minutes(val n: Int) : RelativeTime
    data class Hours(val n: Int) : RelativeTime
    data object Yesterday : RelativeTime
    data class Days(val n: Int) : RelativeTime
}

/** Pure bucketing (unit-testable): < 1 min now, < 1 h minutes, < 24 h hours, < 48 h yesterday, else days. Future = now. */
fun relativeTimeBucket(ageMs: Long): RelativeTime {
    val min = ageMs / 60_000
    return when {
        min < 1 -> RelativeTime.Now
        min < 60 -> RelativeTime.Minutes(min.toInt())
        min < 24 * 60 -> RelativeTime.Hours((min / 60).toInt())
        min < 48 * 60 -> RelativeTime.Yesterday
        else -> RelativeTime.Days((min / (24 * 60)).toInt())
    }
}

/** Localised relative time of [epochMs] against [nowMs]. */
@Composable
fun relativeTime(epochMs: Long, nowMs: Long = System.currentTimeMillis()): String = when (val b = relativeTimeBucket(nowMs - epochMs)) {
    RelativeTime.Now -> stringResource(R.string.ui_time_now)
    is RelativeTime.Minutes -> stringResource(R.string.ui_time_min_ago, b.n)
    is RelativeTime.Hours -> stringResource(R.string.ui_time_h_ago, b.n)
    RelativeTime.Yesterday -> stringResource(R.string.ui_time_yesterday)
    is RelativeTime.Days -> stringResource(R.string.ui_time_d_ago, b.n)
}
