// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import android.app.Application
import app.tatakai.next.model.LinkRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module

/**
 * Koin bindings of Tatakai Link. Requires `androidContext()`. The key is sealed under `noBackupFilesDir` (review E4).
 * parity wave A (2026-11-09 amendment): [LinkHistoryRemote] is the history peer consumed by :core:data's `HistorySyncer` (worker q).
 * parity wave B (16 §3.11, worker r): mDNS discovery ([NsdPcDiscovery]), the foreground gate of the `GET /v1/events` long-poll
 * ([AppForeground]) and the "Enviar a dispositivo" notification ([AndroidLinkHandoffNotifier]); [LinkDiscoveryAccess] for :feature:link.
 */
val linkModule: Module = module {
    single { LinkKeyVault(androidContext().noBackupFilesDir, AndroidKeystoreKey.cipher()) }
    single {
        val app = androidContext().applicationContext as? Application
        LinkRepositoryImpl(
            vault = get(),
            pcDiscovery = NsdPcDiscovery(androidContext()),
            foreground = app?.let { AppForeground(it).visible },
            handoffNotifier = AndroidLinkHandoffNotifier(androidContext()),
        )
    }
    single<LinkRepository> { get<LinkRepositoryImpl>() }
    single<RemotePlaybackAccess> { get<LinkRepositoryImpl>() }
    single<LinkDiscoveryAccess> { get<LinkRepositoryImpl>() }
    single<LinkHistoryRemote> { LinkHistoryRemoteImpl(get<LinkRepositoryImpl>()) }
}
