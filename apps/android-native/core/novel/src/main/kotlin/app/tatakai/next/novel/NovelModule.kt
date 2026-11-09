// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.novel

import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.ChapterDownloadSource
import app.tatakai.next.model.Narrator
import app.tatakai.next.model.NovelRepository
import app.tatakai.next.model.SettingsRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.binds
import org.koin.dsl.module
import java.io.File

/**
 * Koin bindings of novels (docs/redesign/16-parity-wave-b-contract-android.md §3.8). Requires `androidContext()`, dataModule
 * (MetadataRepository, ListsRepository, SettingsRepository, SecretStore), extensionsModule (ExtensionRepository), networkModule (OkHttpClient).
 */
val novelModule: Module = module {
    single {
        val settings = get<SettingsRepository>()
        NovelRepositoryImpl(
            metadata = get(),
            extensions = get(),
            lists = getOrNull(),
            client = get(),
            storeDir = File(androidContext().filesDir, NovelRepositoryImpl.DIR_NAME),
            settings = { settings.settings.value },
        )
    } binds arrayOf(AppDataWiper::class, ChapterDownloadSource::class)
    single<NovelRepository> { get<NovelRepositoryImpl>() }
    single<Narrator> {
        val settings = get<SettingsRepository>()
        NarratorImpl(
            context = androidContext(),
            client = get(),
            secrets = get(),
            settings = { settings.settings.value.tts },
        )
    }
}
