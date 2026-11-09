plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.coil.compose)
    // android wave (12-android-wave-contract.md): SourcePickerSheet/SourceRow (worker d) and rememberDownloadGate (worker f)
    implementation(projects.feature.player)
    implementation(projects.feature.torrents)
    // parity wave A (docs/redesign/15-parity-wave-a-contract-android.md §3.3): ListButton (worker q) mounted in the Details header
    implementation(projects.feature.library)
    // parity wave A (15 §5 o.2, worker o): BackHandler of the in-place related-title stack.
    implementation(libs.androidx.activity.compose)
}

dependencies {
    // parity wave B (docs/redesign/16-parity-wave-b-contract-android.md §3.1 / §3.2 / §3.7): the three slots of WaveBSlots —
    // FollowShowRow (r, :feature:home), DownloadSeasonSlot (r, :feature:downloads), AniListSocialSlot (t, :feature:social). No cycle:
    // none of the three depends on :feature:details.
    implementation(projects.feature.home)
    implementation(projects.feature.downloads)
    implementation(projects.feature.social)
}
