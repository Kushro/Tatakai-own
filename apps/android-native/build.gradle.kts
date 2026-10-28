// Root build. Conventions live in build-logic/ (tatakai.* plugins). The version catalog is FROZEN after B1:
// new dependencies are requested in the module's NOTES-deps.md and added by the integrator.
plugins {
    alias(libs.plugins.android.application) apply false
    alias(libs.plugins.android.library) apply false
    alias(libs.plugins.kotlin.android) apply false
    alias(libs.plugins.kotlin.jvm) apply false
    alias(libs.plugins.kotlin.compose) apply false
    alias(libs.plugins.kotlin.serialization) apply false
    alias(libs.plugins.ksp) apply false
}

// Material3 Adaptive 1.3.0 (ListDetailSceneStrategy for Navigation 3) is built with AGP 9.1 and declares minAgpVersion=9.1.0,
// plus compileSdk 37. The project stays on AGP 8.13.2 (same toolchain as Miko), so the AAR-metadata gate is disabled in every module.
subprojects {
    tasks.configureEach {
        if (name.startsWith("check") && name.endsWith("AarMetadata")) enabled = false
    }
}
