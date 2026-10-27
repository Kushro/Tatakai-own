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
