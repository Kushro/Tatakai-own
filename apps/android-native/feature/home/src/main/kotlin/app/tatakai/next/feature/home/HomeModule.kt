package app.tatakai.next.feature.home

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val homeModule = module {
    viewModel { HomeViewModel(metadata = get(), history = get()) }
}
