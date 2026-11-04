// android wave contract (docs/redesign/12-android-wave-contract.md), owner: worker f.
// Torrent client screens (Library tab, detail Overview/Files/Peers/Trackers, Downloads + "Entiendo" warning, Torrent settings page).
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.compose.material.icons.extended)
    implementation(libs.material3.adaptive)
    implementation(libs.material3.adaptive.layout)
    implementation(libs.androidx.activity.compose)
    implementation(libs.coil.compose)
}
