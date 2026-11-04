package app.tatakai.next.feature.search

import org.koin.android.ext.koin.androidContext
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val searchModule = module {
    single<SearchHistoryStore> { SharedPrefsSearchHistoryStore(androidContext()) }
    viewModel { SearchViewModel(metadata = get(), historyStore = get(), extensions = get()) }
}
