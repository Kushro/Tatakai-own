// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents

import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.remember

// android wave contract (docs/redesign/12 §3.12). Owner: worker f; used by worker c (Details "Descargar") and anyone else who starts a
// background download. Name and signature FIXED.

/** Gate in front of `TorrentRepository.addDownload` (flow 11 §4.3). */
@Stable
interface DownloadGate {
    /**
     * If background downloads are acknowledged and enabled, runs [onReady] immediately. Otherwise shows the full-screen "Entiendo /
     * Ahora no" warning (not a dismissable dialog), stores the ack, enables background downloads, asks POST_NOTIFICATIONS (13+) with
     * the explanation, then runs [onReady]. "Ahora no" runs nothing.
     */
    fun launch(onReady: () -> Unit)
}

/** Remember a gate bound to the current composition (it draws its own warning UI when needed). Body: worker f. */
@Composable
fun rememberDownloadGate(): DownloadGate = remember {
    object : DownloadGate {
        override fun launch(onReady: () -> Unit) = onReady()
    }
}
