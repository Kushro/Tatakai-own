// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.SettingsRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.bind
import org.koin.dsl.module

/**
 * Koin bindings of the web downloads (docs/redesign/16-parity-wave-b-contract-android.md §3.8). Requires `androidContext()`,
 * dataModule (SettingsRepository, HistoryRepository, MetadataRepository, ListsRepository), extensionsModule (ExtensionRepository).
 * Worker r adds here: `single<DownloadManager>` (+ `StandaloneDatabaseProvider`, `SimpleCache`, data source factory) consumed by
 * [TatakaiDownloadService], and the `AutoDownloadWorker.schedule / cancel` reaction to `AppSettings.downloads.autoCheck`.
 */
val downloadModule: Module = module {
    single {
        val settings = get<SettingsRepository>()
        DownloadRepositoryImpl(
            context = androidContext(),
            settings = { settings.settings.value.downloads },
            extensions = get(),
            metadata = get(),
            history = get(),
            lists = getOrNull(),
        )
    } bind AppDataWiper::class
    single<DownloadRepository> { get<DownloadRepositoryImpl>() }
}
