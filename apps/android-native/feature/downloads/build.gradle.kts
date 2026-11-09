// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.2), owner: worker r.
// Downloads page (web + torrents + chapters + local library), "Descargar temporada" sheet mounted in Details, Settings › Descargas.
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.coil.compose)
    implementation(libs.androidx.activity.compose) // SAF pickers (import files, choose folder)
    implementation(libs.androidx.documentfile)
}
