package app.tatakai.next

import android.app.Application
import app.tatakai.next.data.dataModule
import app.tatakai.next.database.databaseModule
import app.tatakai.next.extensions.extensionsModule
import app.tatakai.next.feature.details.detailsModule
import app.tatakai.next.feature.extensions.extensionsFeatureModule
import app.tatakai.next.feature.home.homeModule
import app.tatakai.next.feature.library.libraryModule
import app.tatakai.next.feature.link.linkFeatureModule
import app.tatakai.next.feature.search.searchModule
import app.tatakai.next.feature.settings.settingsModule
import app.tatakai.next.link.linkModule
import app.tatakai.next.metadata.metadataModule
import app.tatakai.next.network.networkModule
import app.tatakai.next.player.playerModule
import app.tatakai.next.supabase.PROP_SUPABASE_ANON_KEY
import app.tatakai.next.supabase.PROP_SUPABASE_URL
import app.tatakai.next.supabase.supabaseModule
import org.koin.android.ext.koin.androidContext
import org.koin.core.context.startKoin

/**
 * Koin starts here, before WorkManager's default initializer runs (HistorySyncWorker is a KoinComponent), so the
 * default initializer must stay enabled. Supabase config comes from BuildConfig (E8), never from committed files.
 */
class TatakaiApplication : Application() {
    override fun onCreate() {
        super.onCreate()
        startKoin {
            androidContext(this@TatakaiApplication)
            properties(
                mapOf(
                    PROP_SUPABASE_URL to BuildConfig.TATAKAI_SUPABASE_URL,
                    PROP_SUPABASE_ANON_KEY to BuildConfig.TATAKAI_SUPABASE_ANON_KEY,
                ),
            )
            modules(
                // B3 - data
                networkModule, metadataModule, databaseModule, supabaseModule, dataModule,
                // B4 - media
                extensionsModule, playerModule, linkModule,
                // B2 - feature ViewModels
                homeModule, searchModule, detailsModule, libraryModule, settingsModule,
                extensionsFeatureModule, linkFeatureModule,
            )
        }
    }
}
