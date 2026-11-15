// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.format

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import app.tatakai.next.ui.R
import java.time.Instant
import java.time.LocalDate
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.temporal.ChronoUnit

// 2026-11 UI refresh: episode dates as "hace 1 semana" / "en 6 días" (desktop episodes view, Hayase-like), calendar headers.

/** What to say about a calendar day relative to today (pure, unit-tested). Negative = past. */
sealed interface RelativeDay {
    data object Today : RelativeDay
    data object Tomorrow : RelativeDay
    data object Yesterday : RelativeDay

    /** [n] days ahead (2..6) or ago (-2..-6). */
    data class Days(val n: Int) : RelativeDay

    /** [n] weeks ahead / ago (1..4). */
    data class Weeks(val n: Int) : RelativeDay

    /** [n] months ahead / ago (1..11). */
    data class Months(val n: Int) : RelativeDay

    /** [n] years ahead / ago (>= 1). */
    data class Years(val n: Int) : RelativeDay
}

/** Buckets [days] = date − today: 0 today, ±1 tomorrow / yesterday, < 7 days, < 30 weeks, < 365 months, else years. */
fun relativeDayBucket(days: Long): RelativeDay {
    val a = kotlin.math.abs(days)
    val sign = if (days < 0) -1 else 1
    return when {
        days == 0L -> RelativeDay.Today
        days == 1L -> RelativeDay.Tomorrow
        days == -1L -> RelativeDay.Yesterday
        a < 7 -> RelativeDay.Days(sign * a.toInt())
        a < 30 -> RelativeDay.Weeks(sign * (a / 7).toInt())
        a < 365 -> RelativeDay.Months(sign * (a / 30).coerceIn(1, 11).toInt())
        else -> RelativeDay.Years(sign * (a / 365).toInt())
    }
}

/**
 * Parses a provider air date (`2023-09-29`, `2023-09-29T14:00:00Z`, `2023-09-29T14:00:00+09:00`) to the local calendar day; null when
 * it is not a date.
 */
fun parseAirDay(raw: String?, zone: ZoneId = ZoneId.systemDefault()): LocalDate? {
    val s = raw?.trim().orEmpty()
    if (s.isEmpty()) return null
    return runCatching { Instant.parse(s).atZone(zone).toLocalDate() }.getOrNull()
        ?: runCatching { OffsetDateTime.parse(s).atZoneSameInstant(zone).toLocalDate() }.getOrNull()
        ?: runCatching { LocalDate.parse(s.take(10)) }.getOrNull()
}

/** Days from [today] to [day] (negative = past). */
fun daysFrom(today: LocalDate, day: LocalDate): Long = ChronoUnit.DAYS.between(today, day)

/** "hoy", "mañana", "hace 1 semana", "en 6 días"... */
@Composable
fun relativeDay(bucket: RelativeDay): String = when (bucket) {
    RelativeDay.Today -> stringResource(R.string.ui_day_today)
    RelativeDay.Tomorrow -> stringResource(R.string.ui_day_tomorrow)
    RelativeDay.Yesterday -> stringResource(R.string.ui_day_yesterday)
    is RelativeDay.Days -> plural(bucket.n, R.plurals.ui_days_in, R.plurals.ui_days_ago)
    is RelativeDay.Weeks -> plural(bucket.n, R.plurals.ui_weeks_in, R.plurals.ui_weeks_ago)
    is RelativeDay.Months -> plural(bucket.n, R.plurals.ui_months_in, R.plurals.ui_months_ago)
    is RelativeDay.Years -> plural(bucket.n, R.plurals.ui_years_in, R.plurals.ui_years_ago)
}

/** Relative day of a provider air date, null when it cannot be parsed. */
@Composable
fun relativeAirDay(raw: String?, today: LocalDate = LocalDate.now()): String? =
    parseAirDay(raw)?.let { relativeDay(relativeDayBucket(daysFrom(today, it))) }

@Composable
private fun plural(n: Int, future: Int, past: Int): String {
    val a = kotlin.math.abs(n)
    return if (n >= 0) pluralStringResource(future, a, a) else pluralStringResource(past, a, a)
}
