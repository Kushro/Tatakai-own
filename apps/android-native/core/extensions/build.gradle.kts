// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.jvm.library") }

dependencies {
    api(projects.core.model)
    implementation(libs.kotlinx.serialization.json)
    implementation(libs.kotlinx.coroutines.core)
}
