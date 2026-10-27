package app.tatakai.next.feature.search

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val searchModule = module {
    viewModel { SearchViewModel(metadata = get()) }
}
