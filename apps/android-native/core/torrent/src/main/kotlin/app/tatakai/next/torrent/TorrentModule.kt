// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent

import app.tatakai.next.model.torrent.TorrentRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module

/**
 * Koin bindings of the torrent engine (name FIXED: :app lists it): the libtorrent4j SessionManager wrapper, loopback HTTP server,
 * labels.json, TorrentService control and network / battery / storage policies, all behind [TorrentRepository]. The concrete type is
 * also bound (internal) so [TorrentService] reaches the same instance.
 */
val torrentModule: Module = module {
    single { TorrentRepositoryImpl(context = androidContext(), settingsRepository = get()) }
    single<TorrentRepository> { get<TorrentRepositoryImpl>() }
}
