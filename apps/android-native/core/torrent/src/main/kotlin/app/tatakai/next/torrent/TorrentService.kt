// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent

import androidx.lifecycle.LifecycleService

/**
 * Sole owner of the torrent session while anything streams or downloads (11 §5.2). Foreground type `mediaPlayback` while a loopback
 * stream is open, `dataSync` for background downloads; Android 15 `onTimeout` pauses everything and posts "Descargas pausadas por el
 * sistema". Notification channel id [CHANNEL_ID]. Declared in this module's AndroidManifest.xml. Body: worker f.
 */
class TorrentService : LifecycleService() {
    companion object {
        const val CHANNEL_ID: String = "torrents"
    }
}
