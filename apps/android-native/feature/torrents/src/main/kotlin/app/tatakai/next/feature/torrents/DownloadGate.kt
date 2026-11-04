// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents

import android.Manifest
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import app.tatakai.next.feature.torrents.ui.BackgroundWarning
import app.tatakai.next.feature.torrents.ui.rememberNeedsNotificationPermission
import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.launch
import org.koin.compose.koinInject

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

/** Remember a gate bound to the current composition (it draws its own warning UI when needed). */
@Composable
fun rememberDownloadGate(): DownloadGate {
    val settingsRepository = koinInject<SettingsRepository>()
    val scope = rememberCoroutineScope()
    val needsPermission = rememberNeedsNotificationPermission()
    var pending by remember { mutableStateOf<(() -> Unit)?>(null) }
    var afterPermission by remember { mutableStateOf<(() -> Unit)?>(null) }
    val permissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {
        // granted or not, the download goes ahead (the foreground notification is just hidden without permission)
        afterPermission?.invoke()
        afterPermission = null
    }
    val currentRepository by rememberUpdatedState(settingsRepository)

    val gate = remember {
        object : DownloadGate {
            override fun launch(onReady: () -> Unit) {
                val torrent = currentRepository.settings.value.torrent
                if (torrent.backgroundAck && torrent.backgroundDownloads) onReady() else pending = onReady
            }
        }
    }

    pending?.let { action ->
        Dialog(
            onDismissRequest = { pending = null }, // back = "Ahora no"
            properties = DialogProperties(usePlatformDefaultWidth = false, dismissOnClickOutside = false, decorFitsSystemWindows = false),
        ) {
            Surface(Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.surface) {
                BackgroundWarning(
                    onAccept = {
                        pending = null
                        scope.launch {
                            try {
                                currentRepository.acknowledgeBackgroundDownloads()
                                val t = currentRepository.settings.value.torrent
                                currentRepository.setTorrentSettings(t.copy(backgroundAck = true, backgroundDownloads = true))
                            } catch (e: CancellationException) {
                                throw e
                            } catch (_: Exception) {
                                // the download call reports "background off" through its own Result
                            }
                            if (needsPermission()) {
                                afterPermission = action
                                permissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
                            } else {
                                action()
                            }
                        }
                    },
                    onNotNow = { pending = null },
                )
            }
        }
    }
    return gate
}
