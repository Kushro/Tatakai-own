package app.tatakai.next.feature.extensions

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val extensionsFeatureModule = module {
    viewModel { ExtensionsViewModel(repo = get()) }
    // parity wave B (16 §3.4, worker t): hub sections (updates, repositories, Stremio, providers, preferences, kill switch) and provider pages.
    viewModel { ExtensionsHubViewModel(repo = get(), stremio = getOrNull(), settings = getOrNull()) }
    viewModel { params -> ProviderViewModel(extensionId = params.get<String>(0), sourceId = params.get<String>(1), repo = get()) }
    viewModel { params -> ProviderItemViewModel(extensionId = params.get<String>(0), sourceId = params.get<String>(1), itemId = params.get<String>(2), repo = get(), history = getOrNull()) }
}
