// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.5), owner: worker s.
// Manga home, details, chapter list, reader (paged / vertical / webtoon), readlist tab of Library, Settings › Lector.
plugins { id("tatakai.android.feature") }

dependencies {
    // MangaCatalog / chapter logic / keymap of the readers (core module, no feature -> feature dependency).
    implementation(projects.core.manga)
    implementation(libs.coil.compose)
    // NetworkHeaders / httpHeaders for page images that need the extension's Referer (coil-network-core through the OkHttp fetcher).
    implementation(libs.coil.network.okhttp)
    implementation(libs.androidx.activity.compose) // BackHandler
    implementation(libs.androidx.core.ktx) // WindowCompat (immersive reader)
    implementation(libs.compose.material.icons.extended)
}
