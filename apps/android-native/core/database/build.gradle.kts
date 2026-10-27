// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins {
    id("tatakai.android.library")
    alias(libs.plugins.ksp)
}

dependencies {
    implementation(projects.core.model)
    implementation(libs.room.runtime)
    implementation(libs.room.ktx)
    ksp(libs.room.compiler)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
