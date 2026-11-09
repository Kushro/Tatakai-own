// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * Parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.9, worker q; checklist torrent.magnet-link-handler).
 *
 * A `magnet:` URI or a `.torrent` content URI the system handed to the app (intent filters of MainActivity in :app). MainActivity
 * offers it here and navigates to `Route.Torrents(LIBRARY)` through the existing `TorrentLink` flow; the Torrents screen (q) collects
 * [link], opens the "Añadir torrent" sheet pre-filled (`TorrentRepository.addManual`) and clears it with [consume]. Bound in `torrentModule`.
 */
class PendingMagnetLink {
    private val _link = MutableStateFlow<String?>(null)
    val link: StateFlow<String?> = _link.asStateFlow()

    fun offer(raw: String) {
        _link.value = raw
    }

    /** Returns and clears the pending link. */
    fun consume(): String? = _link.value.also { _link.value = null }

    companion object {
        const val MAGNET_SCHEME = "magnet"
        const val TORRENT_MIME = "application/x-bittorrent"
    }
}
