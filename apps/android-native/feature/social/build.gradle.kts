// parity wave B contract (docs/redesign/16-parity-wave-b-contract-android.md §3.7), owner: worker t.
// AniList profile (own + public), followers / following, activity feed + thread, notifications, the reviews / activity slot of Details.
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.coil.compose)
    // parity wave B (worker t): SocialActivityLookup (thread screen), POST_NOTIFICATIONS request, ContextCompat.
    implementation(projects.core.social)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.core.ktx)
}
