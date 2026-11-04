// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.you

import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier

// android wave contract (docs/redesign/12 §3.13). Owner: worker b. Name and signature FIXED.

/**
 * "Tú" (prototype android.js `S.you`): account card (avatar, email or guest, last sync, "Sincronizar ahora" / "Iniciar sesión") and the
 * list Historial · Descargas · Extensiones · Dispositivo enlazado · Ajustes with one-line status each. Body: worker b.
 */
@Composable
fun YouScreen(
    onOpenHistory: () -> Unit,
    onOpenDownloads: () -> Unit,
    onOpenExtensions: () -> Unit,
    onOpenLinked: () -> Unit,
    onOpenSettings: () -> Unit,
    onSignIn: () -> Unit,
    modifier: Modifier = Modifier,
) {
    Text("YouScreen", modifier)
}
