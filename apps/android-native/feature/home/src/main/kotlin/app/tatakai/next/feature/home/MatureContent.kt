// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.home

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.NoAdultContent
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshots.SnapshotStateList
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.tatakai.next.model.MatureContentMode
import app.tatakai.next.model.Media
import app.tatakai.next.ui.media.LocalTitleLanguage
import app.tatakai.next.ui.media.MediaCard
import app.tatakai.next.ui.media.displayTitle
import app.tatakai.next.ui.media.mediaKey

/**
 * Parity wave B (worker r, checklist settings.privacy.mature-content): how Home presents adult titles once `showAdult` lets them in.
 * SHOW = as any title; BLUR = blurred poster until tapped once; WARN = confirmation dialog before opening, once per title (remembered for
 * the life of the screen state, like the classic gate).
 */
class MatureGate internal constructor(
    val mode: MatureContentMode,
    private val accepted: SnapshotStateList<String>,
    private val ask: (Media, () -> Unit) -> Unit,
) {
    fun isHidden(media: Media): Boolean = mode != MatureContentMode.SHOW && HomeLogic.isMature(media) && mediaKey(media) !in accepted

    /** Runs [open] directly, after revealing (BLUR) or after the dialog (WARN). */
    fun open(media: Media, open: () -> Unit) {
        if (!isHidden(media)) return open()
        when (mode) {
            MatureContentMode.BLUR -> accepted += mediaKey(media)
            MatureContentMode.WARN -> ask(media, open)
            MatureContentMode.SHOW -> open()
        }
    }

    internal fun accept(media: Media) {
        accepted += mediaKey(media)
    }
}

@Composable
fun rememberMatureGate(mode: MatureContentMode): MatureGate {
    val accepted = remember { mutableStateListOf<String>() }
    var pending by remember { mutableStateOf<Pair<Media, () -> Unit>?>(null) }
    val gate = remember(mode) { MatureGate(mode, accepted) { media, open -> pending = media to open } }
    pending?.let { (media, open) ->
        AlertDialog(
            onDismissRequest = { pending = null },
            icon = { Icon(Icons.Outlined.NoAdultContent, contentDescription = null) },
            title = { Text(stringResource(R.string.home_mature_title)) },
            text = { Text(stringResource(R.string.home_mature_body, media.displayTitle(LocalTitleLanguage.current))) },
            confirmButton = {
                TextButton(onClick = {
                    gate.accept(media)
                    pending = null
                    open()
                }) { Text(stringResource(R.string.home_mature_continue)) }
            },
            dismissButton = { TextButton(onClick = { pending = null }) { Text(stringResource(app.tatakai.next.ui.R.string.ui_cancel)) } },
        )
    }
    return gate
}

/** [MediaCard] that honours [gate]: blurred poster + label while hidden in BLUR mode. */
@Composable
fun GatedMediaCard(media: Media, gate: MatureGate, onClick: () -> Unit, modifier: Modifier = Modifier, width: Dp = 120.dp) {
    val hidden = gate.isHidden(media) && gate.mode == MatureContentMode.BLUR
    Box(modifier.width(width)) {
        MediaCard(media = media, onClick = { gate.open(media, onClick) }, modifier = Modifier.fillMaxWidth().then(if (hidden) Modifier.blur(18.dp) else Modifier))
        if (hidden) {
            Column(Modifier.align(Alignment.Center).padding(8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Icon(Icons.Outlined.NoAdultContent, contentDescription = null, tint = MaterialTheme.colorScheme.onSurface)
                Text(stringResource(R.string.home_mature_tap), style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurface)
            }
        }
    }
}
