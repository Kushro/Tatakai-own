plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.bundles.camerax)
    implementation(libs.zxing.core)
    implementation(libs.androidx.activity.compose)
    // docs/redesign/13 §3.10: connect screen of the PC extension servers
    implementation(projects.core.extensionServer)
}
