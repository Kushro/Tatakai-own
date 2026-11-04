// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.player

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.automirrored.rounded.OpenInNew
import androidx.compose.material.icons.rounded.OndemandVideo
import androidx.compose.material.icons.rounded.Stop
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

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
    ExternalPlayerContent(title, appLabel, onStop, onBack, modifier, torrentLine, onReopen = null)
}

/** [ExternalPlayerScreen] plus the player's private "Open again" action (the external app was closed without leaving). */
@Composable
internal fun ExternalPlayerContent(
    title: String,
    appLabel: String?,
    onStop: () -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    torrentLine: String? = null,
    onReopen: (() -> Unit)? = null,
) {
    val scheme = MaterialTheme.colorScheme
    Surface(modifier.fillMaxSize(), color = scheme.background, contentColor = scheme.onBackground) {
        Column(
            Modifier.fillMaxSize().padding(24.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp, Alignment.CenterVertically),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Icon(Icons.Rounded.OndemandVideo, contentDescription = null, tint = scheme.primary, modifier = Modifier.size(64.dp))
            Text(
                if (appLabel != null) stringResource(R.string.player_external_title, appLabel) else stringResource(R.string.player_external_title_generic),
                style = MaterialTheme.typography.headlineSmall,
                textAlign = TextAlign.Center,
            )
            Text(title, style = MaterialTheme.typography.titleMedium, color = scheme.onSurfaceVariant, textAlign = TextAlign.Center)
            if (torrentLine != null) {
                Text(torrentLine, style = MaterialTheme.typography.labelLarge, fontFamily = FontFamily.Monospace, color = scheme.onSurfaceVariant)
            }
            Text(
                stringResource(R.string.player_external_hint),
                style = MaterialTheme.typography.bodyMedium,
                color = scheme.onSurfaceVariant,
                textAlign = TextAlign.Center,
                modifier = Modifier.widthIn(max = 480.dp),
            )
            Spacer(Modifier.height(8.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                Button(onClick = onStop) {
                    Icon(Icons.Rounded.Stop, contentDescription = null)
                    Text(stringResource(R.string.player_external_stop), Modifier.padding(start = 8.dp))
                }
                OutlinedButton(onClick = onBack) {
                    Icon(Icons.AutoMirrored.Rounded.ArrowBack, contentDescription = null)
                    Text(stringResource(R.string.player_back), Modifier.padding(start = 8.dp))
                }
            }
            if (onReopen != null) {
                TextButton(onClick = onReopen) {
                    Icon(Icons.AutoMirrored.Rounded.OpenInNew, contentDescription = null, modifier = Modifier.size(18.dp))
                    Text(stringResource(R.string.player_external_reopen), Modifier.padding(start = 8.dp))
                }
            }
        }
    }
}
