// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
// parity wave A (2026-11-09 amendment): :core:supabase is gone; the history syncs over Tatakai Link (:core:link, LinkHistoryRemote).
plugins { id("tatakai.android.library") }

dependencies {
    api(projects.core.model)
    implementation(projects.core.network)
    implementation(projects.core.metadata)
    implementation(projects.core.database)
    implementation(projects.core.link)
    implementation(projects.core.release)
    implementation(libs.room.runtime)
    implementation(libs.work.runtime)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
