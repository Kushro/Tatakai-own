// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.downloads

import app.tatakai.next.model.ChapterDownloadSource
import org.koin.core.module.dsl.viewModel
import org.koin.dsl.module

/** Parity wave B (16 §3.8, worker r). `getAll<ChapterDownloadSource>()` = the manga and novel sources bound by s (empty until then). */
val downloadsFeatureModule = module {
    viewModel { DownloadsViewModel(downloads = get(), torrents = getOrNull(), chapters = getAll<ChapterDownloadSource>()) }
}
