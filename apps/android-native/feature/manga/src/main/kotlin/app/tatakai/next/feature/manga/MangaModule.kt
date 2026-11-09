// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.manga

import app.tatakai.next.model.MangaReaderArgs
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/**
 * Parity wave B (16 §3.8, worker s): ViewModels of the manga screens. The name is FINAL (TatakaiApplication). Parameters: (anilistId: Int)
 * for the details page, (args: MangaReaderArgs) for the reader. Needs mangaModule (MangaRepository, MangaCatalog) and dataModule.
 */
val mangaFeatureModule = module {
    viewModel { MangaHomeViewModel(repo = get(), catalog = get(), settingsRepo = get()) }
    viewModel { params -> MangaDetailsViewModel(anilistId = params.get<Int>(), repo = get(), catalog = get(), settingsRepo = get()) }
    viewModel { params -> MangaReaderViewModel(args = params.get<MangaReaderArgs>(), repo = get(), catalog = get(), settingsRepo = get()) }
    viewModel { MangaLibraryViewModel(repo = get(), catalog = get(), lists = getOrNull()) }
}
