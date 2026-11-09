// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.torrents

import app.tatakai.next.feature.torrents.vm.AddTorrentViewModel
import app.tatakai.next.feature.torrents.vm.DebridViewModel
import app.tatakai.next.feature.torrents.vm.TorrentDetailViewModel
import app.tatakai.next.feature.torrents.vm.TorrentHistoryViewModel
import app.tatakai.next.feature.torrents.vm.TorrentDownloadsViewModel
import app.tatakai.next.feature.torrents.vm.TorrentLibraryViewModel
import app.tatakai.next.feature.torrents.vm.TorrentSettingsViewModel
import org.koin.core.module.Module
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/** ViewModels of the torrent client (name FIXED: :app lists it). TorrentDetailViewModel takes the torrent id as parameter. */
val torrentsFeatureModule: Module = module {
    viewModel { TorrentLibraryViewModel(repository = get()) }
    viewModel { params -> TorrentDetailViewModel(torrentId = params.get<String>(), repository = get()) }
    viewModel { TorrentDownloadsViewModel(repository = get(), settingsRepository = get()) }
    viewModel { TorrentSettingsViewModel(settingsRepository = get(), repository = get()) }
    // parity wave A (15 §3.9, worker q)
    viewModel { AddTorrentViewModel(repository = get(), metadata = getOrNull(), pending = getOrNull()) }
    viewModel { TorrentHistoryViewModel(repository = get()) }
    viewModel { DebridViewModel(debrid = getOrNull()) }
}
