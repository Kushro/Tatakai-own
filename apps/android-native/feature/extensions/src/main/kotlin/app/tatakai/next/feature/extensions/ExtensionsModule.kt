package app.tatakai.next.feature.extensions

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val extensionsFeatureModule = module {
    viewModel { ExtensionsViewModel(repo = get()) }
}
