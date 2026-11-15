package app.tatakai.next.ui.media

import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.runtime.remember
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import app.tatakai.next.model.CardStyle
import app.tatakai.next.model.Density
import app.tatakai.next.ui.look.LocalLook
import app.tatakai.next.ui.look.pressScale
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
import app.tatakai.next.model.TitleLanguage
import java.util.Locale

/**
 * Poster + title (+ subtitle, + optional watch progress). Primitive overload so history entries can reuse it.
 * 2026-11 UI refresh: [style] follows Diseño y estilo ([LocalLook]); [CardStyle.MINIMAL] shows the poster only (the title stays
 * for TalkBack), the other two the title and [subtitle] ([CardStyle.DETAILED] adds the [statusDot] before the title).
 */
@Composable
fun MediaCard(
    title: String,
    coverUrl: String?,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    subtitle: String? = null,
    progress: Float? = null,
    style: CardStyle = LocalLook.current.cardStyle,
    statusDot: Color? = null,
) {
    val interaction = remember { MutableInteractionSource() }
    val compact = LocalLook.current.density == Density.COMPACT
    Column(
        modifier
            .pressScale(interaction)
            .clip(MaterialTheme.shapes.medium)
            .clickable(interactionSource = interaction, indication = LocalIndication.current, onClick = onClick)
            .then(if (style == CardStyle.MINIMAL) Modifier.semantics(mergeDescendants = true) { contentDescription = title } else Modifier)
            .padding(bottom = if (style == CardStyle.MINIMAL) 0.dp else 4.dp),
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
        if (style == CardStyle.MINIMAL) return@Column
        Row(
            Modifier.padding(top = if (compact) 4.dp else 6.dp, start = 2.dp, end = 2.dp),
            verticalAlignment = Alignment.Top,
        ) {
            if (style == CardStyle.DETAILED && statusDot != null) {
                Box(
                    Modifier
                        .padding(top = 6.dp, end = 6.dp)
                        .size(8.dp)
                        .clip(CircleShape)
                        .background(statusDot),
                )
            }
            Text(
                title,
                style = if (compact) MaterialTheme.typography.bodySmall else MaterialTheme.typography.bodyMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
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

/**
 * [Media] overload: the title follows the user's preferred title language ([LocalTitleLanguage], provided by the app shell from
 * Settings › Metadatos) unless [titleLanguage] is passed explicitly. The fact line follows [style]: classic "2024 · 12 ep",
 * detailed status dot + "2024 · TV".
 */
@Composable
fun MediaCard(
    media: Media,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    titleLanguage: TitleLanguage = LocalTitleLanguage.current,
    style: CardStyle = LocalLook.current.cardStyle,
) {
    MediaCard(
        title = media.displayTitle(titleLanguage),
        coverUrl = media.cover,
        onClick = onClick,
        modifier = modifier,
        subtitle = when (style) {
            CardStyle.CLASSIC -> classicFactsLine(media) ?: mediaSubtitle(media)
            CardStyle.DETAILED -> detailedFactsLine(media) ?: mediaSubtitle(media)
            CardStyle.MINIMAL -> null
        },
        style = style,
        statusDot = mediaStatusKind(media.status)?.dotColor(),
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
