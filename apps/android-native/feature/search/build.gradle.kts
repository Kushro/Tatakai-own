// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.android.feature") }

dependencies {
    // parity wave A (docs/redesign/15 §3.1, worker o): TrendingFeed / TrendWindow (Tendencias window selector).
    implementation(projects.core.metadata)
    // Image picker (GetContent) and BackHandler of the in-place character page.
    implementation(libs.androidx.activity.compose)
    // Picked-image and trace.moe preview frames.
    implementation(libs.coil.compose)
    // Character search results open the character page (CharacterScreen of :feature:details) in place: SearchScreen has no
    // character callback in the contract. No cycle: :feature:details does not depend on :feature:search.
    implementation(projects.feature.details)
}
