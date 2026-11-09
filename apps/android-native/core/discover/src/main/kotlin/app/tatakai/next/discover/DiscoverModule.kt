// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.discover

import app.tatakai.next.model.AiringRepository
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.RecsRepository
import app.tatakai.next.model.SettingsRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.bind
import org.koin.dsl.module
import java.io.File

/**
 * Koin bindings of the calendar and the recommendations (docs/redesign/16-parity-wave-b-contract-android.md §3.8). Requires
 * `androidContext()`, dataModule (MetadataRepository, HistoryRepository, ListsRepository, SettingsRepository). Worker r adds here the
 * `AiringReminderWorker.schedule / cancel` reaction to `AppSettings.airingReminders` (collect `SettingsRepository.settings` in an app scope).
 */
val discoverModule: Module = module {
    single {
        val settings = get<SettingsRepository>()
        AiringRepositoryImpl(
            metadata = get(),
            lists = getOrNull(),
            storeDir = File(androidContext().filesDir, AiringRepositoryImpl.DIR_NAME),
            remindersEnabled = { settings.settings.value.airingReminders },
        )
    } bind AppDataWiper::class
    single<AiringRepository> { get<AiringRepositoryImpl>() }
    single {
        RecsRepositoryImpl(
            metadata = get(),
            history = get(),
            lists = getOrNull(),
            storeDir = File(androidContext().filesDir, RecsRepositoryImpl.DIR_NAME),
        )
    } bind AppDataWiper::class
    single<RecsRepository> { get<RecsRepositoryImpl>() }
}
