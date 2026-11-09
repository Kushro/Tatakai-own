// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.download

import android.content.Context
import androidx.media3.exoplayer.offline.DownloadManager
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.DownloadRepository
import app.tatakai.next.model.SettingsRepository
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.launch
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.bind
import org.koin.dsl.module

/**
 * Koin bindings of the web downloads (docs/redesign/16-parity-wave-b-contract-android.md §3.8). Requires `androidContext()`,
 * dataModule (SettingsRepository, HistoryRepository, MetadataRepository, ListsRepository), extensionsModule (ExtensionRepository,
 * EmbedSniffer, read lazily). `single<DownloadManager>` is the manager of [DownloadRepositoryImpl] (driven by [TatakaiDownloadService]).
 * [DownloadScheduler] is eager: it applies Settings › Descargas, schedules / cancels [AutoDownloadWorker] following `autoCheck` and runs
 * `verifyIntegrity()` once at app start (checklist downloads.integrity-check).
 */
val downloadModule: Module = module {
    single {
        val settings = get<SettingsRepository>()
        DownloadRepositoryImpl(
            context = androidContext(),
            settings = { settings.settings.value.downloads },
            extensions = get(),
            metadata = get(),
            history = get(),
            lists = getOrNull(),
        )
    } bind AppDataWiper::class
    single<DownloadRepository> { get<DownloadRepositoryImpl>() }
    single<DownloadManager> { get<DownloadRepositoryImpl>().manager }
    single(createdAtStart = true) { DownloadScheduler(androidContext(), get(), get()) }
}

/** App-lifetime reactions of :core:download (see [downloadModule]). */
class DownloadScheduler internal constructor(context: Context, settings: SettingsRepository, repo: DownloadRepository) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    init {
        scope.launch {
            settings.settings.map { it.downloads.autoCheck }.distinctUntilChanged().collect { on ->
                runCatching { if (on) AutoDownloadWorker.schedule(context) else AutoDownloadWorker.cancel(context) }
            }
        }
        scope.launch { runCatching { repo.verifyIntegrity() } }
    }
}
