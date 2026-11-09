# NOTES-deps (B3 android-data)

No new dependencies needed beyond the frozen catalog. Wiring contract for `:app` (B2):

- Koin modules: `networkModule, metadataModule, databaseModule, linkModule, dataModule` (parity wave A, 2026-11-09 amendment: the hosted-backend module is
  gone; `:core:data` depends on `:core:link` for `LinkHistoryRemote`).
- Koin properties: `TATAKAI_ANILIST_CLIENT_ID`, `TATAKAI_MAL_CLIENT_ID` (from BuildConfig, optional), optional `tatakai.appVersion`.
  The hosted-backend BuildConfig keys no longer exist.
- WorkManager: `HistorySyncWorker` uses `KoinComponent`, so Koin must be started before WorkManager runs (default initializer is fine);
  do not disable the default WorkManager initializer.
- `:core:metadata` pulls `koin-core` (already in the catalog) for `metadataModule`.

## android wave (12 §4 a)

- No new catalog dependencies. `:core:data` now also depends on `:core:release` (project dependency, for `updateReliability`).
- New Koin bindings in `dataModule`: `SourceStatsStore` (`SourceStatsStoreImpl`, Room tables `source_stats` / `picked_source`, DB version 2,
  destructive migration as before) and `HistorySyncStamp` (last successful sync, key `history_last_synced_ms` in the settings DataStore).
- `MetadataRepositoryImpl` reads `AppSettings.metadataCacheTtlHours` for details / episode lists (feeds keep 15 min).

## parity wave A (15 §3.3-3.4, 2026-11-09 amendment)

- The hosted-backend module was removed (module, its catalog entries and `ktor-client-okhttp`, `checkTatakaiConfig`, its BuildConfig keys).
- New bindings in `dataModule`: `PendingOAuthRedirect`, `SecretStore` (`KeystoreSecretStore`), `ListsRepository`, `LogRepository` (+ `AppLog.sink`),
  `UpdateRepository`, `AppReset`. `HistorySyncer` now takes `LinkHistoryRemote` (bound in `linkModule`) and `AppSettings.historySyncOverLink`.

## parity wave A, worker q

- No new catalog entries. `:core:data` now declares `libs.androidx.core.ktx` (FileProvider of the app log, manifest of this module),
  `libs.okhttp` (already transitive through `:core:network`) and `testImplementation(libs.mockwebserver3)` (already in the catalog).
- Room v3 (lists tables): destructive migration as before.
- New Koin-free helpers: `WorkManagerListSyncScheduler` / `ListSyncWorker` (KoinComponent, same rule as `HistorySyncWorker`).
