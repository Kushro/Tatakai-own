// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

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
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.designsystem.components.ErrorState
import app.tatakai.next.designsystem.components.SkeletonBox
import app.tatakai.next.designsystem.components.rememberAfterDelay
import app.tatakai.next.model.AppError
import app.tatakai.next.model.LoadState
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaSort
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.ui.media.COVER_ASPECT_RATIO
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.state.userMessage
import java.io.File
import app.tatakai.next.ui.R as UiR

internal val NOVEL_CARD_WIDTH = 120.dp

@Composable
internal fun NovelSectionHeader(text: String, action: (@Composable () -> Unit)? = null) {
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 16.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.weight(1f).semantics { heading() })
        action?.invoke()
    }
}

@Composable
internal fun NovelCard(title: String, cover: String?, subtitle: String?, onClick: () -> Unit, progress: Float? = null) {
    Column(Modifier.width(NOVEL_CARD_WIDTH).clickable(onClick = onClick)) {
        Box(contentAlignment = Alignment.BottomCenter) {
            MediaCover(url = cover, modifier = Modifier.fillMaxWidth())
            if (progress != null) LinearProgressIndicator(progress = { progress.coerceIn(0f, 1f) }, modifier = Modifier.fillMaxWidth().padding(8.dp))
        }
        Text(title, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(top = 6.dp))
        subtitle?.let { Text(it, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1) }
    }
}

@Composable
internal fun NovelRow(title: String, state: LoadState<List<Media>>, lang: TitleLanguage, onRetry: () -> Unit, onClick: (Media) -> Unit) {
    when (state) {
        LoadState.Loading -> {
            NovelSectionHeader(title)
            if (rememberAfterDelay()) {
                Row(Modifier.padding(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                    repeat(4) { SkeletonBox(Modifier.width(NOVEL_CARD_WIDTH).height(NOVEL_CARD_WIDTH / COVER_ASPECT_RATIO)) }
                }
            }
        }
        is LoadState.Err -> {
            NovelSectionHeader(title)
            ErrorState(message = state.cause.userMessage(), retryLabel = stringResource(UiR.string.ui_retry), onRetry = onRetry, compact = true)
        }
        is LoadState.Ok -> if (state.value.isNotEmpty()) {
            NovelSectionHeader(title)
            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                items(state.value, key = { it.ref.anilistId ?: it.title.hashCode() }) { m ->
                    NovelCard(m.displayTitle(lang), m.cover, m.year?.toString(), onClick = { onClick(m) })
                }
            }
        }
    }
}

@Composable
internal fun NovelError(cause: AppError, onRetry: () -> Unit, modifier: Modifier = Modifier) {
    ErrorState(message = cause.userMessage(), retryLabel = stringResource(UiR.string.ui_retry), onRetry = onRetry, modifier = modifier, title = stringResource(UiR.string.ui_error_title))
}

@Composable
internal fun <T> NovelFilterDropdown(label: String, value: String?, options: List<T>, optionLabel: @Composable (T) -> String, onSelect: (T) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        FilterChip(selected = value != null, onClick = { open = true }, label = { Text(if (value != null) "$label: $value" else label) })
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { o ->
                DropdownMenuItem(text = { Text(optionLabel(o)) }, onClick = {
                    open = false
                    onSelect(o)
                })
            }
        }
    }
}

@Composable
internal fun novelStatusLabel(status: String?): String? = when (status?.uppercase()) {
    "RELEASING" -> stringResource(R.string.novel_status_releasing)
    "FINISHED" -> stringResource(R.string.novel_status_finished)
    "HIATUS" -> stringResource(R.string.novel_status_hiatus)
    "CANCELLED" -> stringResource(R.string.novel_status_cancelled)
    "NOT_YET_RELEASED" -> stringResource(R.string.novel_status_upcoming)
    null -> null
    else -> status
}

@Composable
internal fun novelOriginLabel(origin: String?): String? = when (origin?.uppercase()) {
    "JP" -> stringResource(R.string.novel_origin_jp)
    "KR" -> stringResource(R.string.novel_origin_kr)
    "CN" -> stringResource(R.string.novel_origin_cn)
    null -> null
    else -> origin
}

@Composable
internal fun novelSortLabel(sort: MediaSort): String = stringResource(
    when (sort) {
        MediaSort.POPULARITY -> R.string.novel_sort_popularity
        MediaSort.TRENDING -> R.string.novel_sort_trending
        MediaSort.SCORE -> R.string.novel_sort_score
        MediaSort.START_DATE -> R.string.novel_sort_newest
        MediaSort.TITLE -> R.string.novel_sort_title
        MediaSort.FAVOURITES -> R.string.novel_sort_favourites
        MediaSort.EPISODES -> R.string.novel_sort_chapters
        MediaSort.RELEVANCE -> R.string.novel_sort_relevance
    },
)

// ---- typography (checklist novel.reader.typography / novel.reader.custom-font) ----

internal data class NovelPalette(val background: Color, val text: Color, val highlight: Color)

/** Reader colours: `theme` follows the app theme; sepia / black / white / gray are fixed reading palettes. */
@Composable
internal fun novelPalette(background: String): NovelPalette = when (background) {
    "sepia" -> NovelPalette(Color(0xFFF4ECD8), Color(0xFF5B4636), Color(0x33B5835A))
    "black" -> NovelPalette(Color.Black, Color(0xFFDDDDDD), Color(0x33FFFFFF))
    "white" -> NovelPalette(Color.White, Color(0xFF1B1B1B), Color(0x22000000))
    "gray" -> NovelPalette(Color(0xFF2E2E2E), Color(0xFFE2E2E2), Color(0x33FFFFFF))
    else -> NovelPalette(MaterialTheme.colorScheme.surface, MaterialTheme.colorScheme.onSurface, MaterialTheme.colorScheme.primaryContainer.copy(alpha = 0.6f))
}

/**
 * The font of a preset, port of the classic CSS stacks: Georgia and Lora fall back to the system serif (Android ships neither), the
 * dyslexia preset uses the sans family with wider letter spacing ([dyslexicSpacing]); `custom` loads the imported TTF / OTF.
 */
internal fun novelFontFamily(font: String, customPath: String?): FontFamily = when (font) {
    "sans" -> FontFamily.SansSerif
    "serif", "georgia", "lora" -> customPath?.takeIf { font == "lora" && File(it).name.contains("lora", ignoreCase = true) }
        ?.let { p -> runCatching { FontFamily(Font(File(p))) }.getOrNull() } ?: FontFamily.Serif
    "dyslexic" -> FontFamily.SansSerif
    "custom" -> customPath?.let { p -> runCatching { FontFamily(Font(File(p))) }.getOrNull() } ?: FontFamily.Serif
    else -> FontFamily.Default
}

internal fun dyslexicSpacing(font: String): Float = if (font == "dyslexic") 0.06f else 0f

internal val NOVEL_FONTS: List<String> = listOf("serif", "sans", "georgia", "lora", "dyslexic", "system", "custom")

@Composable
internal fun fontLabel(font: String): String = stringResource(
    when (font) {
        "sans" -> R.string.novel_font_sans
        "georgia" -> R.string.novel_font_georgia
        "lora" -> R.string.novel_font_lora
        "dyslexic" -> R.string.novel_font_dyslexic
        "system" -> R.string.novel_font_system
        "custom" -> R.string.novel_font_custom
        else -> R.string.novel_font_serif
    },
)
