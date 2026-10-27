// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins {
    id("tatakai.android.library")
    id("tatakai.android.compose")
}

dependencies {
    api(projects.core.designsystem)
    api(projects.core.model)
    implementation(libs.coil.compose)
    implementation(libs.coil.network.okhttp)
    implementation(libs.material3.adaptive)
    implementation(libs.material3.adaptive.layout)
}
