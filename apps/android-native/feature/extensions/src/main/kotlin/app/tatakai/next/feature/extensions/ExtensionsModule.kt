package app.tatakai.next.feature.extensions

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

val extensionsFeatureModule = module {
    viewModel { ExtensionsViewModel(repo = get()) }
    // parity wave B (16 §3.4, worker t): hub sections (updates, repositories, Stremio, providers, preferences, kill switch) and provider pages.
    viewModel { ExtensionsHubViewModel(repo = get(), stremio = getOrNull(), settings = getOrNull()) }
    // parity wave B closure (ext.custom-providers.continue): Continue watching of a provider title resumes in the player.
    single { ProviderResumer(repo = get()) }
    // parity wave B closure (server.api.extensions): extensions of the linked PC servers (null client = section hidden).
    viewModel { ServerExtensionsViewModel(repository = getOrNull()) }
    viewModel { params -> ProviderViewModel(extensionId = params.get<String>(0), sourceId = params.get<String>(1), repo = get()) }
    viewModel { params -> ProviderItemViewModel(extensionId = params.get<String>(0), sourceId = params.get<String>(1), itemId = params.get<String>(2), repo = get(), history = getOrNull()) }
}
