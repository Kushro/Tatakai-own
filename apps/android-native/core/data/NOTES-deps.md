# NOTES-deps (B3 android-data)

No new dependencies needed beyond the frozen catalog. Wiring contract for `:app` (B2):

- Koin modules: `networkModule, metadataModule, databaseModule, supabaseModule, dataModule`.
- Koin properties: `TATAKAI_SUPABASE_URL`, `TATAKAI_SUPABASE_ANON_KEY` (from BuildConfig), optional `tatakai.appVersion`.
- WorkManager: `HistorySyncWorker` uses `KoinComponent`, so Koin must be started before WorkManager runs (default initializer is fine);
  do not disable the default WorkManager initializer.
- `:core:metadata` pulls `koin-core` (already in the catalog) for `metadataModule`.
