// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.2), owner: worker r.
// Web downloads (HLS / MP4) over the Media3 DownloadManager + TatakaiDownloadService (foreground, dataSync), auto-download worker
// (WorkManager), local library (SAF). Everything below is already in the catalog; new dependencies go to NOTES-deps.md of this module.
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    // release-name parser for the local library import / scan matching
    implementation(projects.core.release)
    implementation(libs.bundles.media3)
    implementation(libs.media3.database)
    implementation(libs.media3.datasource)
    implementation(libs.work.runtime)
    implementation(libs.okhttp)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.documentfile)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
    testImplementation(libs.mockwebserver3)
}
