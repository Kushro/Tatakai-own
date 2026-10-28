// B4 (m3e/android-media): player screen. Media3-Compose / M3E APIs used here were checked against .deps-sources (review E2).
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(projects.core.player)
    implementation(projects.core.link)   // RemotePlaybackAccess (Link HLS + heartbeat)
    implementation(libs.bundles.media3)
    implementation(libs.media3.ui.compose)
    implementation(libs.androidx.activity.compose)
    implementation(libs.compose.material.icons.extended)
}
