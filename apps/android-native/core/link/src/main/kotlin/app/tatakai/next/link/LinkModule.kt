// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.link

import app.tatakai.next.model.LinkRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module

/**
 * Koin bindings of Tatakai Link. Requires `androidContext()`. The key is sealed under `noBackupFilesDir` (review E4).
 * parity wave A (2026-11-09 amendment): [LinkHistoryRemote] is the history peer consumed by :core:data's `HistorySyncer` (worker q).
 */
val linkModule: Module = module {
    single { LinkKeyVault(androidContext().noBackupFilesDir, AndroidKeystoreKey.cipher()) }
    single { LinkRepositoryImpl(vault = get()) }
    single<LinkRepository> { get<LinkRepositoryImpl>() }
    single<RemotePlaybackAccess> { get<LinkRepositoryImpl>() }
    single<LinkHistoryRemote> { LinkHistoryRemoteImpl(get<LinkRepositoryImpl>()) }
}
