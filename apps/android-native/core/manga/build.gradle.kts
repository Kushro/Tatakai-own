// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.5), owner: worker s.
// Manga: metadata extras (AniList / MangaDex / MangaUpdates), chapters through `.kai` custom `read` sources, reading progress,
// readlist (local + AniList progress), chapter downloads (pages on disk). Dependencies already in the catalog; new ones -> NOTES-deps.md.
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    implementation(libs.okhttp)
    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.documentfile)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
    testImplementation(libs.mockwebserver3)
}
