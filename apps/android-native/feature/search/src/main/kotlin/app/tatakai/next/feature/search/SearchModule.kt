package app.tatakai.next.feature.search

import app.tatakai.next.metadata.TrendingFeed
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val searchModule = module {
    single<SearchHistoryStore> { SharedPrefsSearchHistoryStore(androidContext()) }
    // parity wave A (15 §3.1, worker o): image search reads the picked / shared picture through the ContentResolver.
    single<ImageBytesReader> { ContentImageReader(androidContext()) }
    viewModel {
        // TrendingFeed lives in metadataModule (:core:metadata); getOrNull keeps Search usable if it is not loaded.
        val trending = getOrNull<TrendingFeed>()
        SearchViewModel(
            metadata = get(),
            historyStore = get(),
            extensions = get(),
            settings = get(),
            trending = trending?.let { feed -> TrendingSource { window, page, adult -> feed.page(window, page, adult) } },
        )
    }
    viewModel { ImageSearchViewModel(metadata = get(), reader = get()) }
}
