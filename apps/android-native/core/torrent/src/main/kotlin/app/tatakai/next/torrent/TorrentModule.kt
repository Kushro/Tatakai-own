// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.torrent

import org.koin.core.module.Module
import org.koin.dsl.module

/**
 * Koin bindings of the torrent engine (name FIXED: :app lists it). Worker f binds `single<TorrentRepository> { ... }` here
 * (libtorrent4j SessionManager wrapper, loopback HTTP server, labels.json, TorrentService control, network/battery/storage policies).
 */
val torrentModule: Module = module {}
