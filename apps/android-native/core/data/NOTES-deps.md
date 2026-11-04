# NOTES-deps (B3 android-data)

No new dependencies needed beyond the frozen catalog. Wiring contract for `:app` (B2):

- Koin modules: `networkModule, metadataModule, databaseModule, supabaseModule, dataModule`.
- Koin properties: `TATAKAI_SUPABASE_URL`, `TATAKAI_SUPABASE_ANON_KEY` (from BuildConfig), optional `tatakai.appVersion`.
- WorkManager: `HistorySyncWorker` uses `KoinComponent`, so Koin must be started before WorkManager runs (default initializer is fine);
  do not disable the default WorkManager initializer.
- `:core:metadata` pulls `koin-core` (already in the catalog) for `metadataModule`.

## android wave (12 §4 a)

- No new catalog dependencies. `:core:data` now also depends on `:core:release` (project dependency, for `updateReliability`).
- New Koin bindings in `dataModule`: `SourceStatsStore` (`SourceStatsStoreImpl`, Room tables `source_stats` / `picked_source`, DB version 2,
  destructive migration as before) and `HistorySyncStamp` (last successful sync, key `history_last_synced_ms` in the settings DataStore).
- `MetadataRepositoryImpl` reads `AppSettings.metadataCacheTtlHours` for details / episode lists (feeds keep 15 min).
