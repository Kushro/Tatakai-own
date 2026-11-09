// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import androidx.lifecycle.ViewModel
import app.tatakai.next.model.ChapterDownloadSource
import app.tatakai.next.model.DownloadItem
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.torrent.TorrentRepository
import kotlinx.coroutines.flow.StateFlow

/**
 * Parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.2, worker r, checklist downloads.page): one state for the six tabs
 * (`DownloadsTab`): web downloads from [downloads], torrents from [torrents] (library + background downloads of :core:torrent, finished by
 * q), chapters from every [chapters] source (manga / novels of s), the local library, search + ETA + progress. Stub: exposes the raw flows.
 */
class DownloadsViewModel(
    private val downloads: DownloadRepository,
    private val torrents: TorrentRepository? = null,
    private val chapters: List<ChapterDownloadSource> = emptyList(),
) : ViewModel() {
    val items: StateFlow<List<DownloadItem>> = downloads.items
}
