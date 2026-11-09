// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.android.feature") }

dependencies {
    // add feature-specific project/library dependencies here
    // android wave (12-android-wave-contract.md): TorrentSettingsPage (worker f)
    implementation(projects.feature.torrents)
}

dependencies {
    // android wave (worker b): BackHandler for the Torrents subpage (already in the catalog).
    implementation(libs.androidx.activity.compose)
}

dependencies {
    // parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.2 / §3.3):
    // PlayerSettingsExtras (worker p's rows inside Settings › Reproductor), DiscoverSettingsExtras (worker o's rows inside Metadatos)
    // and Custom Tabs for the AniList / MAL OAuth (worker q).
    implementation(projects.feature.player)
    implementation(projects.feature.search)
    implementation(libs.androidx.browser)
}

dependencies {
    // parity wave A (worker q): PendingOAuthRedirect / ListErrors / OAuthRedirect of :core:data (Listas y cuentas) and the
    // FileProvider of "Abrir registros" (androidx.core, already in the catalog). No cycle: :core:data depends on no feature module.
    implementation(projects.core.data)
    implementation(libs.androidx.core.ktx)
}

dependencies {
    // parity wave A closure (worker q, checklist system.clear-cache): "Limpiar caché" also clears Coil's image disk cache through
    // SingletonImageLoader (coil-compose is already in the catalog and used by core/ui, home, details, search).
    implementation(libs.coil.compose)
}

dependencies {
    // parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.9): DownloadsSettingsSection (r), ReaderSettingsSection /
    // NovelReaderSettingsSection (s), SourcesSettingsExtras (r, :feature:extensions). No cycle: none of them depends on :feature:settings.
    implementation(projects.feature.downloads)
    implementation(projects.feature.manga)
    implementation(projects.feature.novel)
    implementation(projects.feature.extensions)
}
