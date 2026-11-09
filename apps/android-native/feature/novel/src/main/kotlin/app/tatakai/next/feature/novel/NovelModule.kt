// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.novel

import app.tatakai.next.model.NovelReaderArgs
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/**
 * Parity wave B (16 §3.8, worker s): ViewModels of the novel screens. The name is FINAL (TatakaiApplication). Parameters: (anilistId: Int)
 * for the details page, (args: NovelReaderArgs) for the reader. Needs novelModule (NovelRepositoryImpl, Narrator), mangaModule
 * (MangaCatalog: AniList facts of the title) and dataModule.
 */
val novelFeatureModule = module {
    viewModel { NovelHomeViewModel(repo = get(), settingsRepo = get()) }
    viewModel { params -> NovelDetailsViewModel(anilistId = params.get<Int>(), repo = get(), catalog = get(), settingsRepo = get()) }
    viewModel { params -> NovelReaderViewModel(args = params.get<NovelReaderArgs>(), repo = get(), narratorApi = get(), settingsRepo = get()) }
    viewModel { NovelLibraryViewModel(repo = get(), lists = getOrNull()) }
}
