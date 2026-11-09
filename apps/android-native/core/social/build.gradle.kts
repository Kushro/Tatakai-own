// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.7), owner: worker t.
// AniList social (profile, follow, activity, notifications by polling, reviews) over graphql.anilist.co with the token of :core:data's
// ListsRepository. No own backend. Dependencies already in the catalog; new ones -> NOTES-deps.md.
plugins { id("tatakai.android.library") }

dependencies {
    implementation(projects.core.model)
    implementation(libs.okhttp)
    implementation(libs.work.runtime)
    implementation(libs.androidx.core.ktx)
    implementation(libs.kotlinx.coroutines.android)
    implementation(libs.kotlinx.serialization.json)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
    testImplementation(libs.mockwebserver3)
}
