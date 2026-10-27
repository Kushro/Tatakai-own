// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.jvm.library") }

dependencies {
    api(libs.kotlinx.serialization.json)
    api(libs.kotlinx.coroutines.core)
}
