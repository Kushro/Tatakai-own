// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent

import app.tatakai.next.model.torrent.DebridRepository
import app.tatakai.next.model.torrent.TorrentRepository
import app.tatakai.next.torrent.debrid.DebridRepositoryImpl
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module

/**
 * Koin bindings of the torrent engine (name FIXED: :app lists it): the libtorrent4j SessionManager wrapper, loopback HTTP server,
 * labels.json, TorrentService control and network / battery / storage policies, all behind [TorrentRepository]. The concrete type is
 * also bound (internal) so [TorrentService] reaches the same instance.
 * parity wave A (15 §3.9, worker q): [DebridRepository] (shared `OkHttpClient` of `networkModule` + `SecretStore` of `dataModule`) and
 * [PendingMagnetLink] (system `magnet:` / `.torrent` handler).
 */
val torrentModule: Module = module {
    single { TorrentRepositoryImpl(context = androidContext(), settingsRepository = get()) }
    single<TorrentRepository> { get<TorrentRepositoryImpl>() }
    single<DebridRepository> { DebridRepositoryImpl(client = get(), secrets = get()) }
    single { PendingMagnetLink() }
}
