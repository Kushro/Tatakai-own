plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.coil.compose)
    // android wave (12-android-wave-contract.md): SourcePickerSheet/SourceRow (worker d) and rememberDownloadGate (worker f)
    implementation(projects.feature.player)
    implementation(projects.feature.torrents)
    // parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.3): ListButton (worker q) mounted in the Details header
    implementation(projects.feature.library)
}
