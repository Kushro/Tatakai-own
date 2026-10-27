package app.tatakai.next.data

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.preferencesDataStoreFile
import app.tatakai.next.metadata.ProviderPriority
import app.tatakai.next.model.AuthRepository
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.SettingsRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module

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

    single<MetadataRepository> { MetadataRepositoryImpl(get(), get()) }

    single<HistorySyncScheduler> { WorkManagerHistorySyncScheduler(androidContext()) }
    single { HistorySyncer(get(), get(), get()) }
    single<HistoryRepository> { HistoryRepositoryImpl(get(), get()) }

    single<AuthRepository> { AuthRepositoryImpl(get(), get(), get(), get()) }
}
