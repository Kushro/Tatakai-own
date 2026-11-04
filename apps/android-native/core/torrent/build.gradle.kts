// android wave contract (docs/redesign/12-android-wave-contract.md), owner: worker f.
// libtorrent4j 2.1.0-39 session + loopback HTTP streaming server + TorrentService (foreground) + TorrentRepository impl (Koin torrentModule).
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    implementation(projects.core.release)
    implementation(libs.bundles.libtorrent4j)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.lifecycle.service)
    implementation(libs.androidx.documentfile)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
