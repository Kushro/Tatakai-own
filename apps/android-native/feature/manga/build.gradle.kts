// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.5), owner: worker s.
// Manga home, details, chapter list, reader (paged / vertical / webtoon), readlist tab of Library, Settings › Lector.
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.coil.compose)
    implementation(libs.androidx.activity.compose) // BackHandler, SAF (nothing else from activity)
}
