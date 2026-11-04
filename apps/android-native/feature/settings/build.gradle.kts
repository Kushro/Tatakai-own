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
