// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.you

import org.koin.core.module.Module
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/** ViewModels of the You screen (name FIXED: :app lists it). The torrent repository is optional (torrentModule may be absent). */
val youFeatureModule: Module = module {
    viewModel {
        YouViewModel(
            auth = get(),
            history = get(),
            extensions = get(),
            link = get(),
            torrents = getOrNull(),
        )
    }
}
