package app.tatakai.next

import android.app.Application
import app.tatakai.next.data.HistorySyncScheduler
import app.tatakai.next.data.PROP_ANILIST_CLIENT_ID
import app.tatakai.next.data.PROP_MAL_CLIENT_ID
import app.tatakai.next.data.dataModule
import app.tatakai.next.database.databaseModule
import app.tatakai.next.extensions.extensionsModule
import app.tatakai.next.extensionserver.extensionServerModule
import app.tatakai.next.feature.details.detailsModule
import app.tatakai.next.feature.extensions.extensionsFeatureModule
import app.tatakai.next.feature.home.homeModule
import app.tatakai.next.feature.library.libraryModule
import app.tatakai.next.feature.link.linkFeatureModule
import app.tatakai.next.feature.search.searchModule
import app.tatakai.next.feature.settings.settingsModule
import app.tatakai.next.feature.torrents.torrentsFeatureModule
import app.tatakai.next.feature.you.youFeatureModule
import app.tatakai.next.link.linkModule
import app.tatakai.next.metadata.metadataModule
import app.tatakai.next.network.networkModule
import app.tatakai.next.player.HistoryFlush
import app.tatakai.next.player.mpv.mpvPlayerModule
import app.tatakai.next.player.playerModule
import app.tatakai.next.torrent.torrentModule
import org.koin.android.ext.koin.androidContext
import org.koin.core.context.startKoin
import org.koin.dsl.module

/** Player -> data bridge: flush pending watch history on pause / exit. */
private val appModule = module {
    single<HistoryFlush> { HistoryFlush { get<HistorySyncScheduler>().requestSync(immediate = true) } }
}

/**
 * Koin starts here, before WorkManager's default initializer runs (HistorySyncWorker is a KoinComponent), so the
 * default initializer must stay enabled. parity wave A, 2026-11-09 amendment: there is no own login and no hosted backend; the only
 * injected configuration is the pair of optional OAuth client ids (BuildConfig, never from committed files).
 */
class TatakaiApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        startKoin {
            androidContext(this@TatakaiApplication)
            properties(
                mapOf(
                    // parity wave A (15 §3.3): list provider OAuth client ids (worker q's ListsRepositoryImpl reads them with getProperty)
                    PROP_ANILIST_CLIENT_ID to BuildConfig.TATAKAI_ANILIST_CLIENT_ID,
                    PROP_MAL_CLIENT_ID to BuildConfig.TATAKAI_MAL_CLIENT_ID,
                ),
            )
            modules(
                // B3 - data (linkModule before dataModule: HistorySyncer consumes LinkHistoryRemote)
                networkModule, metadataModule, databaseModule, linkModule, dataModule,
                // B4 - media
                extensionsModule, playerModule,
                // B2 - feature ViewModels
                homeModule, searchModule, detailsModule, libraryModule, settingsModule,
                extensionsFeatureModule, linkFeatureModule,
                appModule,
                // android wave contract (docs/redesign/12 §3.16)
                torrentModule, mpvPlayerModule, torrentsFeatureModule, youFeatureModule,
                // integrated extension server client (docs/redesign/13 §3.10)
                extensionServerModule,
            )
        }
    }
}
