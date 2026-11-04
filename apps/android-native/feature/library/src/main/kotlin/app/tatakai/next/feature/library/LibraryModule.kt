package app.tatakai.next.feature.library

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val libraryModule = module {
    // TorrentRepository is bound by torrentModule (worker f); getOrNull keeps the Library usable without it (no badge).
    viewModel { LibraryViewModel(history = get(), torrents = getOrNull()) }
}
