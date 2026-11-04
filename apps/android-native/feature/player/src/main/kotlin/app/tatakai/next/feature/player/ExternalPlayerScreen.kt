// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

// android wave contract (docs/redesign/12 §3.11). Owner: worker d. Name and signature FIXED.

/**
 * Informative screen of the EXTERNAL engine (desktop external mpv mode): "Reproduciendo en <app>" with Stop and Back (both end the
 * external session and leave the player). [torrentLine] = speed / peers / % line while a torrent streams. Body: worker d.
 */
@Composable
fun ExternalPlayerScreen(
    title: String,
    appLabel: String?,
    onStop: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    torrentLine: String? = null,
) {
    Text("ExternalPlayerScreen", modifier)
}
