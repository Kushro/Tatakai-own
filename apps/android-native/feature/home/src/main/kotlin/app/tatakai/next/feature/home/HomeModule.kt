package app.tatakai.next.feature.home

import app.tatakai.next.metadata.TrendWindow
import app.tatakai.next.metadata.TrendingFeed
import app.tatakai.next.model.AiringRepository
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.RecsRepository
import app.tatakai.next.model.torrent.TorrentRepository
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val homeModule = module {
    viewModel {
        val trend = getOrNull<TrendingFeed>()
        HomeViewModel(
            metadata = get(),
            history = get(),
            settings = get(),
            extensions = get(),
            // parity wave B (16 §3.1, worker r): every new source is optional (getOrNull) so Home still works without them.
            airing = getOrNull<AiringRepository>(),
            recs = getOrNull<RecsRepository>(),
            trending = trend?.let { t ->
                HomeTrendSource { window, adult ->
                    val w = when (window) {
                        TopWindow.TODAY -> TrendWindow.TODAY
                        TopWindow.WEEK -> TrendWindow.WEEK
                        TopWindow.MONTH -> TrendWindow.MONTH
                    }
                    t.page(w, 1, adult).items
                }
            },
            downloads = getOrNull<DownloadRepository>(),
            torrents = getOrNull<TorrentRepository>(),
        )
    }
    viewModel { CalendarViewModel(airing = get(), lists = getOrNull()) }
    viewModel { ForYouViewModel(recs = get()) }
}
