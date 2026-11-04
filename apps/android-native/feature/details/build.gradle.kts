plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.coil.compose)
    // android wave (12-android-wave-contract.md): SourcePickerSheet/SourceRow (worker d) and rememberDownloadGate (worker f)
    implementation(projects.feature.player)
    implementation(projects.feature.torrents)
}
