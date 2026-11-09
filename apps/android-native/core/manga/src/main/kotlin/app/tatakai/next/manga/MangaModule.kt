// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.manga

import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.ChapterDownloadSource
import app.tatakai.next.model.MangaRepository
import app.tatakai.next.model.SettingsRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.binds
import org.koin.dsl.module
import java.io.File

/**
 * Koin bindings of manga (docs/redesign/16-parity-wave-b-contract-android.md §3.8). Requires `androidContext()`, dataModule
 * (MetadataRepository, ListsRepository, SettingsRepository), extensionsModule (ExtensionRepository). The repository is also the
 * [ChapterDownloadSource] of manga (Downloads page of r, `getAll<ChapterDownloadSource>()`), an [AppDataWiper] and the [MangaCatalog] of
 * the reader screens.
 */
val mangaModule: Module = module {
    single {
        val settings = get<SettingsRepository>()
        MangaRepositoryImpl(
            metadata = get(),
            extensions = get(),
            lists = getOrNull(),
            client = get(),
            storeDir = File(androidContext().filesDir, MangaRepositoryImpl.DIR_NAME),
            settings = { settings.settings.value },
        )
    } binds arrayOf(AppDataWiper::class, ChapterDownloadSource::class)
    single<MangaRepository> { get<MangaRepositoryImpl>() }
    // Extras of the manga / novel screens (AniList facts, MangaUpdates, prefs, reliability), private to the readers.
    single<MangaCatalog> { get<MangaRepositoryImpl>() }
}
