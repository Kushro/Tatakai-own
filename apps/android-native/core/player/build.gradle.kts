// B4 (m3e/android-media): Media3 player core. Depends on :core:link only for RemotePlaybackAccess (mTLS client of the paired PC).
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    implementation(projects.core.link)
    implementation(libs.bundles.media3)
    implementation(libs.okhttp)
    implementation(libs.kotlinx.coroutines.android)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
