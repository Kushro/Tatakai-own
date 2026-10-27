// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(projects.core.player)
    implementation(libs.bundles.media3)
    implementation(libs.media3.ui.compose)
}
