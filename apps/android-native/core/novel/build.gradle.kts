// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.6), owner: worker s.
// Light novels: home / details through metadata (AniList MANGA format NOVEL), chapters through `.kai` custom `novel` sources, progress,
// readlist, chapter text downloads, narration (system TextToSpeech; OpenAI / ElevenLabs / OpenAI-compatible audio played with Media3).
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    // Shared private clients and stores of the readers (AniList manga API, ReadlistStore, JsonFileStore, chapter logic).
    api(projects.core.manga)
    implementation(libs.okhttp)
    implementation(libs.jsoup)
    implementation(libs.media3.common)
    implementation(libs.media3.exoplayer)
    implementation(libs.androidx.core.ktx)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
    testImplementation(libs.mockwebserver3)
}
