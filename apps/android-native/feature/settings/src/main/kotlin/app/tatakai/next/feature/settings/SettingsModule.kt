package app.tatakai.next.feature.settings

import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/**
 * Every collaborator but the settings repository is optional (getOrNull) so Settings never crashes when a module is not bound.
 * 2026-11-09 amendment: no `AuthRepository`; identity comes from `ListsRepository`, history sync from `LinkRepository` + `HistoryRepository`.
 */
val settingsModule = module {
    viewModel {
        SettingsViewModel(
            settingsRepo = get(),
            history = getOrNull(),
            metadata = getOrNull(),
            torrents = getOrNull(),
            lists = getOrNull(),
            link = getOrNull(),
            reset = getOrNull(),
        )
    }
}
