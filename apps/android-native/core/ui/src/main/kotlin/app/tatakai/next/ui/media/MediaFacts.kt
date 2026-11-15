// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.ui.media

import androidx.compose.runtime.Composable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import app.tatakai.next.model.Media
import app.tatakai.next.ui.R

// 2026-11 UI refresh: facts shared by the detailed cards and the Home spotlight (status dot, format, "2024 · 12 ep").

/** Status of a provider status string, for the dot / chip colors (desktop: blue airing, orange finished, violet upcoming). */
enum class MediaStatusKind { AIRING, FINISHED, UPCOMING, CANCELLED, HIATUS }

fun mediaStatusKind(status: String?): MediaStatusKind? = when (status?.trim()?.uppercase()) {
    "RELEASING" -> MediaStatusKind.AIRING
    "FINISHED" -> MediaStatusKind.FINISHED
    "NOT_YET_RELEASED" -> MediaStatusKind.UPCOMING
    "CANCELLED" -> MediaStatusKind.CANCELLED
    "HIATUS" -> MediaStatusKind.HIATUS
    else -> null
}

/** Dot color: fixed hues readable on light and dark surfaces (same meaning as the desktop dots). */
fun MediaStatusKind.dotColor(): Color = when (this) {
    MediaStatusKind.AIRING -> Color(0xFF4C9BFF)
    MediaStatusKind.FINISHED -> Color(0xFFFF9B42)
    MediaStatusKind.UPCOMING -> Color(0xFFB089FF)
    MediaStatusKind.CANCELLED -> Color(0xFFE5484D)
    MediaStatusKind.HIATUS -> Color(0xFF9A9A9A)
}

@Composable
@ReadOnlyComposable
fun MediaStatusKind.label(): String = stringResource(
    when (this) {
        MediaStatusKind.AIRING -> R.string.ui_status_airing
        MediaStatusKind.FINISHED -> R.string.ui_status_finished
        MediaStatusKind.UPCOMING -> R.string.ui_status_upcoming
        MediaStatusKind.CANCELLED -> R.string.ui_status_cancelled
        MediaStatusKind.HIATUS -> R.string.ui_status_hiatus
    },
)

/** Localised AniList format (`TV`, `MOVIE`, `TV_SHORT`...); the raw value for an unknown one, null when missing. */
@Composable
@ReadOnlyComposable
fun mediaFormatLabel(format: String?): String? {
    val f = format?.trim()?.uppercase()?.takeIf { it.isNotEmpty() } ?: return null
    val res = when (f) {
        "TV" -> R.string.ui_format_tv
        "TV_SHORT" -> R.string.ui_format_tv_short
        "MOVIE" -> R.string.ui_format_movie
        "SPECIAL" -> R.string.ui_format_special
        "OVA" -> R.string.ui_format_ova
        "ONA" -> R.string.ui_format_ona
        "MUSIC" -> R.string.ui_format_music
        "MANGA" -> R.string.ui_format_manga
        "NOVEL" -> R.string.ui_format_novel
        "ONE_SHOT" -> R.string.ui_format_one_shot
        else -> return f.replace('_', ' ')
    }
    return stringResource(res)
}

/** Classic card line: "2024 · 12 ep" (year and/or episode count); null when there is neither. */
@Composable
fun classicFactsLine(media: Media): String? {
    val parts = buildList {
        media.year?.let { add(it.toString()) }
        media.episodes?.takeIf { it > 0 }?.let { add(pluralStringResource(R.plurals.ui_episodes_short, it, it)) }
    }
    return parts.takeIf { it.isNotEmpty() }?.joinToString(" · ")
}

/** Detailed card line: "2024 · TV"; null when there is neither. */
@Composable
fun detailedFactsLine(media: Media): String? {
    val parts = listOfNotNull(media.year?.toString(), mediaFormatLabel(media.format))
    return parts.takeIf { it.isNotEmpty() }?.joinToString(" · ")
}
