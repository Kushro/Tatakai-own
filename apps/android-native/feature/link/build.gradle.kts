// Placeholder from B1 (m3e/foundation). The branch that owns this module may overwrite it freely.
// Dependencies below pre-wire what the module is expected to need; the version catalog itself is frozen (NOTES-deps.md for additions).
plugins { id("tatakai.android.feature") }

dependencies {
    implementation(projects.core.link)
    // QR scan: CameraX + zxing, no Google Play services; always keep the "paste code" alternative (06 #3)
    implementation(libs.bundles.camerax)
    implementation(libs.zxing.core)
}
