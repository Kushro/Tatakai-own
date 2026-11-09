package app.tatakai.next.data

import androidx.datastore.core.DataStore
import androidx.datastore.preferences.core.PreferenceDataStoreFactory
import androidx.datastore.preferences.core.Preferences
import androidx.datastore.preferences.preferencesDataStoreFile
import app.tatakai.next.metadata.ProviderPriority
import app.tatakai.next.model.AppDataWiper
import app.tatakai.next.model.AppLog
import app.tatakai.next.model.AppReset
import app.tatakai.next.model.HistoryRepository
import app.tatakai.next.model.LinkRepository
import app.tatakai.next.model.ListProvider
import app.tatakai.next.model.ListsRepository
import app.tatakai.next.model.LogRepository
import app.tatakai.next.model.MetadataRepository
import app.tatakai.next.model.SecretStore
import app.tatakai.next.model.SettingsRepository
import app.tatakai.next.model.SourceStatsStore
import app.tatakai.next.model.UpdateRepository
import org.koin.android.ext.koin.androidContext
import org.koin.core.module.Module
import org.koin.dsl.module
import java.io.File

/**
 * Wires the repositories. Needs networkModule, metadataModule, databaseModule and linkModule loaded too (the history syncs with the
 * paired PC through `LinkHistoryRemote` of :core:link). 2026-11-09 amendment: no hosted backend, no own login; the Koin properties
 * PROP_ANILIST_CLIENT_ID / PROP_MAL_CLIENT_ID (OAuth client ids) are the only configuration.
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
    single { HistoryCursorStore(get<DataStore<Preferences>>()) }
    single {
        val settings = get<SettingsRepository>()
        val cursor = get<HistoryCursorStore>()
        HistorySyncer(
            get(),
            get(),
            enabled = { settings.settings.value.historySyncOverLink },
            stamp = get<HistorySyncStamp>(),
            cursor = { cursor.value() },
            saveCursor = { cursor.set(it) },
        )
    }
    single<HistoryRepository> {
        val settings = get<SettingsRepository>()
        val syncer = get<HistorySyncer>()
        HistoryRepositoryImpl(
            get(),
            get(),
            syncer,
            lists = getOrNull(),
            autoListProgress = { settings.settings.value.autoListProgress },
            autoListAdd = { settings.settings.value.autoListAdd },
        ).also {
            // parity wave A (worker q): sync at app open and when a PC gets paired (the player's HistoryFlush covers episode ends)
            HistorySyncTriggers(get(), getOrNull<LinkRepository>(), syncer).start()
        }
    }

    // android wave (12 §3.1 / §3.16): reliability + picked source, consumed by :core:extensions-quickjs (e) and the player (d)
    single<SourceStatsStore> { SourceStatsStoreImpl(get()) }

    // parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.3-3.4, worker q): lists + OAuth redirect, app log, update check,
    // secrets, reset. One binding per type (nobody else binds these). Client ids arrive as Koin properties PROP_ANILIST_CLIENT_ID /
    // PROP_MAL_CLIENT_ID. q changes the constructors in these `single`s when implementing (the only edit allowed here).
    single { PendingOAuthRedirect() }
    single<SecretStore> { KeystoreSecretStore(androidContext().noBackupFilesDir) }
    single<ListsRepository> {
        val anilistId = getPropertyOrNull<String>(PROP_ANILIST_CLIENT_ID)
        val malId = getPropertyOrNull<String>(PROP_MAL_CLIENT_ID)
        ListsRepositoryImpl(
            dao = get(),
            secrets = get(),
            anilist = AniListListClient(get()),
            mal = MalListClient(get()),
            clientIds = { p -> if (p == ListProvider.ANILIST) anilistId else malId },
            scheduler = WorkManagerListSyncScheduler(androidContext()),
        )
    }
    single<LogRepository> { AppLogStore(File(androidContext().filesDir, "logs")).also { AppLog.sink = it } }
    single<UpdateRepository> {
        val context = androidContext()
        val settings = get<SettingsRepository>()
        val version = runCatching { context.packageManager.getPackageInfo(context.packageName, 0).versionName }.getOrNull().orEmpty()
        UpdateRepositoryImpl(
            client = get(),
            installedVersion = version,
            enabled = { settings.settings.value.updateCheck },
            store = get<DataStore<Preferences>>(),
        )
    }
    single<AppReset> {
        AppResetImpl(
            context = androidContext(),
            settingsStore = get(),
            database = get(),
            secrets = get(),
            metadata = getOrNull(),
            extensions = getOrNull(),
            link = getOrNull(),
            logs = getOrNull(),
            history = getOrNull(),
            // parity wave B (16 §3.8): private stores of :core:download / :core:discover / :core:manga / :core:novel / :core:social
            wipers = getAll<AppDataWiper>(),
        )
    }
}
