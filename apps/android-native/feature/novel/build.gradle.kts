// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.6), owner: worker s.
// Novel home, details, chapter list, reader with typography + narration controls, readlist tab of Library, Settings › Lector de novela.
plugins { id("tatakai.android.feature") }

dependencies {
    // NovelRepositoryImpl extras, Narrator errors, chapter helpers; core/novel exposes core/manga (MangaCatalog) as api.
    implementation(projects.core.novel)
    implementation(libs.coil.compose)
    implementation(libs.androidx.activity.compose) // BackHandler, SAF (custom font TTF / OTF)
    implementation(libs.androidx.core.ktx) // WindowCompat (immersive reader)
}
