// B4 (m3e/android-media): Media3 player core. Depends on :core:link only for RemotePlaybackAccess (mTLS client of the paired PC).
// parity wave A (15 §3.2, worker p): kotlinx-serialization-json for the subtitle translators / cache, mockwebserver3 for their tests
// (both already in the catalog).
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    implementation(projects.core.link)
    implementation(libs.bundles.media3)
    implementation(libs.okhttp)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
    testImplementation(libs.mockwebserver3)
}
