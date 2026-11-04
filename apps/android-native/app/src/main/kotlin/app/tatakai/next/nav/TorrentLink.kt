package app.tatakai.next.nav

import android.content.Intent
import app.tatakai.next.model.torrent.TorrentSection

/**
 * Deep link from a torrent notification (worker f, `core/torrent` TorrentNotifications): the launcher intent carries the section id
 * (`TorrentSection.id`) and an optional torrent id. The shell turns it into `Route.Torrents(section, torrentId)`.
 */
data class TorrentLink(val section: TorrentSection, val torrentId: String?) {
    companion object {
        /** Same values as `TorrentNotifications.EXTRA_SECTION` / `EXTRA_TORRENT_ID` in :core:torrent. */
        const val EXTRA_SECTION: String = "app.tatakai.next.torrent.SECTION"
        const val EXTRA_TORRENT_ID: String = "app.tatakai.next.torrent.TORRENT_ID"

        fun from(intent: Intent?): TorrentLink? {
            val sectionId = intent?.getStringExtra(EXTRA_SECTION) ?: return null
            val section = TorrentSection.entries.firstOrNull { it.id == sectionId } ?: return null
            val torrentId = intent.getStringExtra(EXTRA_TORRENT_ID)
            // A per-torrent section without an id has nothing to show: fall back to the client's library.
            return if (section.perTorrent && torrentId == null) TorrentLink(TorrentSection.LIBRARY, null) else TorrentLink(section, torrentId)
        }
    }
}
