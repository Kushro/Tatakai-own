package app.tatakai.next.data

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.preferencesDataStoreFile
import app.tatakai.next.metadata.ProviderPriority
import app.tatakai.next.model.AppLog
import app.tatakai.next.model.AuthRepository
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LogRepository
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.UpdateRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module
import java.io.File

/**
 * Wires the repositories. Needs networkModule, metadataModule, databaseModule and supabaseModule loaded too, and the
 * Koin properties TATAKAI_SUPABASE_URL / TATAKAI_SUPABASE_ANON_KEY (see :core:supabase).
 */
val dataModule: Module = module {
    single<DataStore<Preferences>> {
        PreferenceDataStoreFactory.create(produceFile = { androidContext().preferencesDataStoreFile("settings") })
    }
    single<SettingsRepository> { SettingsRepositoryImpl(get()) }
    single { ProviderPriority { get<SettingsRepository>().settings.value.providerPriority } }

    single<MetadataRepository> {
        val settings = get<SettingsRepository>()
        MetadataRepositoryImpl(get(), get(), ttlHours = { settings.settings.value.metadataCacheTtlHours })
    }

    single<HistorySyncScheduler> { WorkManagerHistorySyncScheduler(androidContext()) }
    single { HistorySyncStamp(get<DataStore<Preferences>>()) }
    single { HistorySyncer(get(), get(), get(), get<HistorySyncStamp>()) }
    single<HistoryRepository> { HistoryRepositoryImpl(get(), get(), get<HistorySyncer>()) }

    // android wave (12 §3.1 / §3.16): reliability + picked source, consumed by :core:extensions-quickjs (e) and the player (d)
    single<SourceStatsStore> { SourceStatsStoreImpl(get()) }

    single<AuthRepository> { AuthRepositoryImpl(get(), get(), get(), get()) }

    // parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.3-3.4, worker q): lists + OAuth redirect, app log, update check.
    // One binding per type (nobody else binds these). Client ids arrive as Koin properties PROP_ANILIST_CLIENT_ID / PROP_MAL_CLIENT_ID.
    single { PendingOAuthRedirect() }
    single<ListsRepository> { ListsRepositoryImpl() }
    single<LogRepository> { AppLogStore(File(androidContext().filesDir, "logs")).also { AppLog.sink = it } }
    single<UpdateRepository> { UpdateRepositoryImpl() }
}
