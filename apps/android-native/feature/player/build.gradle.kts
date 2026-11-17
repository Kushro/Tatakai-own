// B4 (m3e/android-media): player screen. Media3-Compose / M3E APIs used here were checked against .deps-sources (review E2).
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(projects.core.player)
    implementation(projects.core.link)   // RemotePlaybackAccess (Link HLS + heartbeat)
    implementation(projects.feature.link) // BUG-022: the Link profile names / list for the in-player profile sheet
    implementation(libs.bundles.media3)
    implementation(libs.media3.ui.compose)
    implementation(libs.androidx.activity.compose)
    implementation(libs.compose.material.icons.extended)
    // android wave (12-android-wave-contract.md): libmpv engine + surface (worker d)
    implementation(projects.core.playerMpv)
    // 0.4.0 (worker D): the phone frame generation state / commands the "Mejoras" panel draws (FrameGenController)
    implementation(projects.core.framegen)
    // 0.4.1: the end screen's blurred poster backdrop (MediaCover of :core:ui keeps a fixed aspect ratio)
    implementation(libs.coil.compose)
}
