// android wave contract (docs/redesign/12-android-wave-contract.md), owner: worker f.
// Torrent client screens (Library tab, detail Overview/Files/Peers/Trackers, Downloads + "Entiendo" warning, Torrent settings page).
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.compose.material.icons.extended)
    implementation(libs.material3.adaptive)
    implementation(libs.material3.adaptive.layout)
    implementation(libs.androidx.activity.compose)
    implementation(libs.coil.compose)
    implementation(libs.androidx.documentfile) // SAF tree name in Settings › Torrents › Almacenamiento
}

dependencies {
    // parity wave A (worker q): PendingMagnetLink, DebridRepositoryImpl (preference order) and the alignment preview rules of :core:torrent.
    // No cycle: :core:torrent depends only on :core:model and :core:release.
    implementation(projects.core.torrent)
}

dependencies {
    // parity wave A (worker q): release parser (episode numbers of the alignment preview, already a dependency of :core:torrent).
    implementation(projects.core.release)
}
