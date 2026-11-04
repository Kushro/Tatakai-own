// android wave contract (docs/redesign/12-android-wave-contract.md), owner: worker d.
// libmpv engine (dev.jdtech.mpv:libmpv 1.0.0, class dev.jdtech.mpv.MPVLib) kept out of :core:player so the native library stays optional.
// API reference: .deps-sources/libmpv-1.0.0/MPVLib.signatures.txt (javap of the AAR; the sources jar is empty).
plugins {
    id("tatakai.android.library")
    id("tatakai.android.compose")
}

dependencies {
    implementation(projects.core.model)
    implementation(projects.core.player)
    implementation(libs.libmpv)
    implementation(libs.bundles.compose.core)
    implementation(libs.kotlinx.coroutines.android)
    implementation(platform(libs.koin.bom))
    implementation(libs.koin.core)
    implementation(libs.koin.android)
}
