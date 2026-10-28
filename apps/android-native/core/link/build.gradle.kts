// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
// Android library (Keystore-sealed key, NsdManager is M1, X509KeyManager); pure protocol parts stay unit-testable on the JVM.
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    implementation(libs.okhttp)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
    testImplementation(libs.mockwebserver3)
}
