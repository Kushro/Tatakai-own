// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.player.mpv

import androidx.compose.foundation.layout.Box
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

/** The video surface of the mpv branch (SurfaceView -> MPVLib.attachSurface). OUR controls are drawn on top by feature/player. Body: worker d. */
@Composable
fun MpvPlayerSurface(engine: MpvEngine, modifier: Modifier = Modifier) {
    Box(modifier) { Text("MpvPlayerSurface") }
}
