// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.ui.media.COVER_ASPECT_RATIO
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.media.mediaSubtitle
import app.tatakai.next.ui.state.userMessage
import app.tatakai.next.ui.R as UiR

// Small pieces shared by the manga screens (home rows, mature blur / warning, AniList labels).

internal val CARD_WIDTH = 120.dp

@Composable
internal fun SectionHeader(text: String, modifier: Modifier = Modifier, action: (@Composable () -> Unit)? = null) {
    Row(modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 16.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f).semantics { heading() })
        action?.invoke()
    }
}

/** Poster card; [blurred] hides the cover until tapped once (MatureContentMode.BLUR). */
@Composable
internal fun MangaCard(
    media: Media,
    titleLanguage: TitleLanguage,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    blurred: Boolean = false,
    subtitle: String? = mediaSubtitle(media),
) {
    var revealed by remember(media.ref) { mutableStateOf(false) }
    val hide = blurred && !revealed
    Column(modifier.width(CARD_WIDTH).clickable { if (hide) revealed = true else onClick() }) {
        Box {
            MediaCover(url = media.cover, modifier = Modifier.fillMaxWidth().then(if (hide) Modifier.blur(24.dp) else Modifier))
            if (hide) {
                Text(
                    stringResource(R.string.manga_mature_tap),
                    style = MaterialTheme.typography.labelSmall,
                    color = MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier.align(Alignment.Center).padding(8.dp),
                )
            }
        }
        Text(media.displayTitle(titleLanguage), style = MaterialTheme.typography.bodyMedium, maxLines = 2, modifier = Modifier.padding(top = 6.dp))
        if (subtitle != null) {
            Text(subtitle, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
        }
    }
}

/** One horizontal row of a [LoadState] (skeleton after 150 ms, compact error with retry, hidden when empty). */
@Composable
internal fun MediaRowSection(
    title: String,
    state: LoadState<List<Media>>,
    titleLanguage: TitleLanguage,
    onRetry: () -> Unit,
    onClick: (Media) -> Unit,
    blurred: (Media) -> Boolean = { false },
) {
    when (state) {
        LoadState.Loading -> {
            SectionHeader(title)
            if (rememberAfterDelay()) SkeletonRow()
        }
        is LoadState.Err -> {
            SectionHeader(title)
            ErrorState(
                message = state.cause.userMessage(),
                retryLabel = stringResource(UiR.string.ui_retry),
                onRetry = onRetry,
                compact = true,
            )
        }
        is LoadState.Ok -> if (state.value.isNotEmpty()) {
            SectionHeader(title)
            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                items(state.value, key = { it.ref.anilistId ?: it.title.hashCode() }) { media ->
                    MangaCard(media, titleLanguage, onClick = { onClick(media) }, blurred = blurred(media))
                }
            }
        }
    }
}

@Composable
internal fun SkeletonRow() {
    Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        repeat(4) {
            SkeletonBox(Modifier.width(CARD_WIDTH).height(CARD_WIDTH / COVER_ASPECT_RATIO))
        }
    }
}

/** WARN mode: one confirmation per title before opening it (classic mature dialog). */
@Composable
internal fun MatureWarningDialog(onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.manga_mature_title)) },
        text = { Text(stringResource(R.string.manga_mature_message)) },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.manga_mature_confirm)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(UiR.string.ui_cancel)) } },
    )
}

internal fun MatureContentMode.blurs(): Boolean = this == MatureContentMode.BLUR

@Composable
internal fun anilistStatusLabel(status: String?): String? = when (status?.uppercase()) {
    "RELEASING" -> stringResource(R.string.manga_status_releasing)
    "FINISHED" -> stringResource(R.string.manga_status_finished)
    "HIATUS" -> stringResource(R.string.manga_status_hiatus)
    "CANCELLED" -> stringResource(R.string.manga_status_cancelled)
    "NOT_YET_RELEASED" -> stringResource(R.string.manga_status_upcoming)
    null -> null
    else -> status
}

@Composable
internal fun originLabel(origin: String?): String? = when (origin?.uppercase()) {
    "JP" -> stringResource(R.string.manga_origin_jp)
    "KR" -> stringResource(R.string.manga_origin_kr)
    "CN" -> stringResource(R.string.manga_origin_cn)
    "TW" -> stringResource(R.string.manga_origin_tw)
    null -> null
    else -> origin
}

@Composable
internal fun sortLabel(sort: MediaSort): String = stringResource(
    when (sort) {
        MediaSort.POPULARITY -> R.string.manga_sort_popularity
        MediaSort.TRENDING -> R.string.manga_sort_trending
        MediaSort.SCORE -> R.string.manga_sort_score
        MediaSort.START_DATE -> R.string.manga_sort_newest
        MediaSort.TITLE -> R.string.manga_sort_title
        MediaSort.FAVOURITES -> R.string.manga_sort_favourites
        MediaSort.EPISODES -> R.string.manga_sort_chapters
        MediaSort.RELEVANCE -> R.string.manga_sort_relevance
    },
)

@Composable
internal fun formatLabel(format: String?): String? = when (format?.uppercase()) {
    "MANGA" -> stringResource(R.string.manga_format_manga)
    "ONE_SHOT" -> stringResource(R.string.manga_format_oneshot)
    "NOVEL" -> stringResource(R.string.manga_format_novel)
    null -> null
    else -> format.lowercase().replaceFirstChar { it.uppercase() }
}

@Composable
internal fun ErrorBlock(cause: app.tatakai.next.model.AppError, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    ErrorState(
        message = cause.userMessage(),
        retryLabel = stringResource(UiR.string.ui_retry),
        onRetry = onRetry,
        modifier = modifier,
        title = stringResource(UiR.string.ui_error_title),
    )
}
