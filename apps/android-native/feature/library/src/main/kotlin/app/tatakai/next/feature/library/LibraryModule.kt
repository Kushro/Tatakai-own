package app.tatakai.next.feature.library

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val libraryModule = module {
    // TorrentRepository is bound by torrentModule (worker f); getOrNull keeps the Library usable without it (no badge).
    viewModel { LibraryViewModel(history = get(), torrents = getOrNull()) }
    // parity wave A (15 §3.3, worker q): Library › Listas / Favoritos (ListsRepository is bound in dataModule).
    viewModel { ListsViewModel(lists = get()) }
}
