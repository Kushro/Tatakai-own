// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.1), owner: worker r.
// Airing calendar + followed shows + local reminders (WorkManager) and the on-device recommendations ("Para ti", atlas).
// Everything below is already in the catalog; new dependencies go to NOTES-deps.md of this module.
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    implementation(libs.work.runtime)
    implementation(libs.androidx.core.ktx)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
