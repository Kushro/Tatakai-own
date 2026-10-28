// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.android.library") }

dependencies {
    api(projects.core.extensions)
    implementation(projects.core.model)
    implementation(libs.quickjs.kt)
    implementation(libs.jsoup)
    implementation(libs.okhttp)
    implementation(libs.androidx.documentfile)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
