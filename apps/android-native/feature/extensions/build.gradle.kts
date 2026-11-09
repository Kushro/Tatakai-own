plugins { id("tatakai.android.feature") }

dependencies {
    implementation(libs.androidx.activity.compose)
    // parity wave B closure (16 §3.4, worker t; server.api.extensions): ExtensionServerRepository.extensions for the PC servers section
    implementation(projects.core.extensionServer)
}
