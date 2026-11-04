package app.tatakai.next.feature.settings

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/** History / metadata / torrent repositories are optional (getOrNull) so Settings never crashes when one module is not bound. */
val settingsModule = module {
    viewModel {
        SettingsViewModel(
            settingsRepo = get(),
            auth = get(),
            history = getOrNull(),
            metadata = getOrNull(),
            torrents = getOrNull(),
        )
    }
}
