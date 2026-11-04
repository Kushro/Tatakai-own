// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.link

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import app.tatakai.next.model.PlayerArgs

// android wave contract (docs/redesign/12 §3.13). Owner: worker b. Name and signature FIXED.

/**
 * "Dispositivo enlazado" (prototype android.js `S.linked`): unpaired -> EmptyState + scan QR (CameraX + zxing, QrScanner.kt) / paste
 * code; paired -> status card (connected / unreachable with error + retry), PC capabilities, enhance profile Segmented
 * (LinkRepository.setPreferredProfile), remote control placeholder ("M1", disabled buttons), forget dialog. Replaces LinkScreen for
 * Route.Link and Route.Linked. Body: worker b.
 */
@Composable
fun LinkedDeviceScreen(
    onBack: () -> Unit,
    onPlay: (PlayerArgs) -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("LinkedDeviceScreen", modifier)
}
