// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.components

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Names and signatures FIXED.

/**
 * Tones (desktop `Tone`, decision 1 of 07 §5.1: tokens have no success/warning roles): NEUTRAL = surfaceContainerHighest,
 * OK = primaryContainer, WARN = tertiaryContainer, ERR = errorContainer, PRIMARY = primary (filled). Also used for reliability bars
 * (>= 0.9 OK, >= 0.7 WARN, else ERR) and torrent state pills (downloading/seeding OK, paused NEUTRAL, error ERR, initializing PRIMARY).
 */
enum class PillTone { NEUTRAL, OK, WARN, ERR, PRIMARY }

/** Small status pill (desktop `Pill`). [plain] = text-only, no container. */
@Composable
fun Pill(
    text: String,
    modifier: Modifier = Modifier,
    tone: PillTone = PillTone.NEUTRAL,
    icon: ImageVector? = null,
    plain: Boolean = false,
) {
    val content = if (plain) tone.accentColor() else tone.contentColor()
    val base = if (plain) {
        modifier
    } else {
        modifier
            .clip(CircleShape)
            .background(tone.containerColor())
            .heightIn(min = 24.dp)
            .padding(horizontal = 10.dp, vertical = 3.dp)
    }
    Row(
        modifier = base,
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        if (icon != null) Icon(icon, contentDescription = null, modifier = Modifier.size(14.dp), tint = content)
        Text(
            text,
            style = MaterialTheme.typography.labelMedium,
            color = content,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** Container role of a tone (additive helper, e.g. for reliability bars). */
@Composable
fun PillTone.containerColor(): Color = when (this) {
    PillTone.NEUTRAL -> MaterialTheme.colorScheme.surfaceContainerHighest
    PillTone.OK -> MaterialTheme.colorScheme.primaryContainer
    PillTone.WARN -> MaterialTheme.colorScheme.tertiaryContainer
    PillTone.ERR -> MaterialTheme.colorScheme.errorContainer
    PillTone.PRIMARY -> MaterialTheme.colorScheme.primary
}

/** Content role on top of [containerColor]. */
@Composable
fun PillTone.contentColor(): Color = when (this) {
    PillTone.NEUTRAL -> MaterialTheme.colorScheme.onSurfaceVariant
    PillTone.OK -> MaterialTheme.colorScheme.onPrimaryContainer
    PillTone.WARN -> MaterialTheme.colorScheme.onTertiaryContainer
    PillTone.ERR -> MaterialTheme.colorScheme.onErrorContainer
    PillTone.PRIMARY -> MaterialTheme.colorScheme.onPrimary
}

/** Strong role of a tone on a plain surface (plain pills, bar fills). */
@Composable
fun PillTone.accentColor(): Color = when (this) {
    PillTone.NEUTRAL -> MaterialTheme.colorScheme.onSurfaceVariant
    PillTone.OK, PillTone.PRIMARY -> MaterialTheme.colorScheme.primary
    PillTone.WARN -> MaterialTheme.colorScheme.tertiary
    PillTone.ERR -> MaterialTheme.colorScheme.error
}
