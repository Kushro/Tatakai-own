// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.android.feature") }

dependencies {
    // add feature-specific project/library dependencies here
    // android wave (12-android-wave-contract.md): TorrentsLibraryTab (worker f)
    implementation(projects.feature.torrents)
}

dependencies {
    // parity wave B (16 §3.5 / §3.6, worker s): MangaLibraryTab / NovelLibraryTab (LibraryTab.MANGA / NOVELS). No cycle.
    implementation(projects.feature.manga)
    implementation(projects.feature.novel)
}
