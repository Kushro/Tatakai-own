package app.tatakai.next.feature.settings

import app.tatakai.next.data.PROP_ANILIST_CLIENT_ID
import app.tatakai.next.data.PROP_MAL_CLIENT_ID
import app.tatakai.next.model.ListProvider
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
    // parity wave A (15 §3.3-3.4, worker q): Listas y cuentas (OAuth redirect from PendingOAuthRedirect), Acerca de / Datos actions,
    // first-run wizard. A provider without a client id (TATAKAI_*_CLIENT_ID unset) shows "Vincular" disabled with the reason.
    viewModel {
        val configured = buildSet {
            if (!getPropertyOrNull<String>(PROP_ANILIST_CLIENT_ID).isNullOrBlank()) add(ListProvider.ANILIST)
            if (!getPropertyOrNull<String>(PROP_MAL_CLIENT_ID).isNullOrBlank()) add(ListProvider.MAL)
        }
        AccountsViewModel(lists = get(), settingsRepo = get(), redirects = getOrNull(), configured = configured)
    }
    viewModel { SystemViewModel(logs = getOrNull(), updates = getOrNull(), history = getOrNull()) }
    viewModel { OnboardingViewModel(settingsRepo = get()) }
}
