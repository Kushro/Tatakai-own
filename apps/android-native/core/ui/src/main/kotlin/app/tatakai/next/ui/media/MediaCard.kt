package app.tatakai.next.ui.media

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.tatakai.next.model.Media
import java.util.Locale

/** Poster + title (+ subtitle, + optional watch progress). Primitive overload so history entries can reuse it. */
@Composable
fun MediaCard(
    title: String,
    coverUrl: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    progress: Float? = null,
) {
    Column(
        modifier
            .clip(MaterialTheme.shapes.medium)
            .clickable(onClick = onClick)
            .padding(bottom = 4.dp),
    ) {
        Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.BottomStart) {
            MediaCover(url = coverUrl, modifier = Modifier.fillMaxWidth())
            if (progress != null) {
                LinearProgressIndicator(
                    progress = { progress.coerceIn(0f, 1f) },
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 8.dp),
                )
            }
        }
        Text(
            title,
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.padding(top = 6.dp, start = 2.dp, end = 2.dp),
        )
        if (subtitle != null) {
            Text(
                subtitle,
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.padding(horizontal = 2.dp),
            )
        }
    }
}

@Composable
fun MediaCard(media: Media, onClick: () -> Unit, modifier: Modifier = Modifier) {
    MediaCard(
        title = media.title,
        coverUrl = media.cover,
        onClick = onClick,
        modifier = modifier,
        subtitle = mediaSubtitle(media),
    )
}

/** "2024 • 8.4" style line; null when there is nothing to show. */
fun mediaSubtitle(media: Media): String? {
    val parts = buildList {
        media.year?.let { add(it.toString()) }
        media.score?.let { add(String.format(Locale.ROOT, "%.1f", it / 10.0)) }
    }
    return parts.takeIf { it.isNotEmpty() }?.joinToString(" • ")
}
