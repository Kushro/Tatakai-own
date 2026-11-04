// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.designsystem.components

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector

// android wave contract (docs/redesign/12 §3.9). Owner: worker b. Names and signatures FIXED.

/**
 * Tones (desktop `Tone`, decision 1 of 07 §5.1: tokens have no success/warning roles): NEUTRAL = surfaceContainerHighest,
 * OK = primaryContainer, WARN = tertiaryContainer, ERR = errorContainer, PRIMARY = primary (filled). Also used for reliability bars
 * (>= 0.9 OK, >= 0.7 WARN, else ERR) and torrent state pills (downloading/seeding OK, paused NEUTRAL, error ERR, initializing PRIMARY).
 */
enum class PillTone { NEUTRAL, OK, WARN, ERR, PRIMARY }

/** Small status pill (desktop `Pill`). [plain] = text-only, no container. Body: worker b. */
@Composable
fun Pill(
    text: String,
    modifier: Modifier = Modifier,
    tone: PillTone = PillTone.NEUTRAL,
    icon: ImageVector? = null,
    plain: Boolean = false,
) {
    Text(text, modifier)
}
