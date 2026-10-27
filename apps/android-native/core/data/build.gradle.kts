// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.android.library") }

dependencies {
    api(projects.core.model)
    implementation(projects.core.network)
    implementation(projects.core.metadata)
    implementation(projects.core.database)
    implementation(projects.core.supabase)
    implementation(libs.room.runtime)
    implementation(libs.work.runtime)
    implementation(libs.androidx.datastore.preferences)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
